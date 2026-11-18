# `a2m-telemetry`

Emitting what the engine did: the step trace and its signature, the capture of artifacts at the level a run asks for, spans and metrics, and the engine's own logs. Everything it emits has passed a `Redact` first. It stores nothing durable: what must be kept, it hands back as redacted bytes for `a2m-ledger` to store.

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 2, Foundations |
| May depend on | `a2m-core` |
| Depends on | `a2m-core`. Outside: `serde`, `serde_json`, and `tracing` for the logging macros and the subscriber interface ([decision 0077](../../decisions/0077-logs-through-the-tracing-crate-with-our-own-subscriber-and-spans-through-a-handle.md)). SHA-256 comes through `a2m-core`'s `Digest`. Still to verify and record: an OpenTelemetry exporter, behind a cargo feature |
| Roadmap packages | Roadmap package 1.2, the step trace and its signature; package 2.1, redaction at capture and the check on the way out; the OpenTelemetry export with the server profile, Phase 6, or earlier if a slice needs it |
| Carries out | Spec section [217](../../spec/2-kernel/217-traces-evidence-and-repair.md); [decision 0037](../../decisions/0037-evidence-events-a-step-trace-and-artifacts-redacted-at-capture.md); [decision 0076](../../decisions/0076-one-trace-record-per-step-and-a-signature-chained-over-record-digests.md); the determinism gate's input in [section 403](../../spec/4-building-tools/403-testing-and-publish-gates.md) |

## Owns

- The step-trace record: its fields, its schema, and the JSON Lines form a trace blob takes.
- The trace signature: which parts of a record enter it, in what order, and how they are hashed.
- Capture: given a capture level, which artifacts are kept, and in what form, before anything is written or sent.
- Redaction at capture, through the `Redact` it is given, and the check on the way out.
- The list of headers an exchange may record, for every pack.
- Spans and metrics, with a closed set of attributes that cannot carry a value.
- The engine's logs: structured, levelled, rotated by size, redacted.
- The incoming trace context a caller sends. OpenTelemetry export, off unless a person switches it on, is not built.

It must never: store evidence, own a table, or write a blob (`a2m-ledger` does); decide a capture level (the plan and policy do; it applies one); put a value, a snapshot, or a secret in a span or a metric; send anything off the machine unless export is switched on; paint over an image (the pack that took it does, because only it knows where the fields are).

## The split with `a2m-ledger`

| Concern | `a2m-telemetry` | `a2m-ledger` |
| --- | --- | --- |
| Run events | Nothing. They are the ledger's history | Writes them |
| Step trace | Builds each record, redacts it, computes the signature, and returns a finished trace as bytes | Seals and stores the bytes as a blob of kind `trace`, with the schema's version, and records the signature on the run |
| Artifacts | Decides from the level whether one is kept; redacts it; returns it as bytes with its kind | Seals and stores it as a blob; gives back the name that goes into the step's record |
| Logs | Writes them to the log files in the data directory | Nothing |
| Spans and metrics | Keeps them in memory | Nothing |

The engine is the caller that joins the two: it asks this crate for a finished piece of evidence and gives it to the ledger. An `EvidenceSink` trait here, implemented by the ledger, was weighed and refused for now: it would add a join to the spine for a call the engine can make itself, and it would let telemetry decide when evidence is written. If streaming a long trace to the store proves necessary, the trait is the answer, with a decision record.

## Interface

As built. The crate's own documentation has every item.

```rust
/// Made once per run attempt by the engine. Holds the redactor, the keyed digest, the level, and the bytes so far.
pub struct TraceBuilder { /* ... */ }
impl TraceBuilder {
    pub fn new(identity: TraceIdentity, level: CaptureLevel, redact: Arc<dyn Redact>, keyed: Arc<dyn KeyedDigest>) -> Self;
    pub fn with_limit(self, bytes: usize) -> Self;         // the size the trace is cut at; DEFAULT_TRACE_LIMIT is 32 MiB
    pub fn level(&self) -> CaptureLevel;
    pub fn is_cut(&self) -> bool;

    /// One record per step. Redacted on entry. Never fails: past the size limit a record is left out
    /// of the bytes and still enters the signature.
    pub fn record(&mut self, step: StepRecord);

    /// Offers an artifact. `Kept` carries redacted, checked bytes for the ledger; `Dropped` says why.
    pub fn artifact(&mut self, offered: OfferedArtifact) -> ArtifactOutcome;

    /// Ends the trace: the JSON Lines bytes, already checked, and the signature.
    pub fn finish(self) -> Result<FinishedTrace, TraceRefused>;
}

pub struct TraceIdentity { pub workspace: WorkspaceId, pub run: RunId, pub attempt: u32, pub operation: OperationName,
                           pub plan: Digest, pub engine: Version, pub registry: Digest }
pub enum CaptureLevel { Minimal, Standard, Full }      // names as section 217
impl CaptureLevel { pub const fn keeps(self, kind: ArtifactKind, at: CapturePoint) -> bool; }

pub struct StepRecord {
    pub pass: u32,                        // which turn of the engine loop, from 0
    pub phase: Phase,                     // Act, Confirm, ReadBack: the list in the plan the step came from
    pub step: u32,                        // its position in that list
    pub kind: StepKind,
    pub record: Option<RecordKey>,        // written only as the workspace's keyed digest
    pub begins: Option<PassStart>,        // on the first record of a pass
    pub took: Duration,                   // not in the signature
    pub locating: Option<Locating>,       // locator, position, score
    pub acting: Option<Acting>,           // means: in the signature; measured: not
    pub values: Vec<TracedValue>,
    pub outcome: TraceOutcome,            // Verified, Done, or Failed { class, code }
    pub artifacts: Vec<ArtifactRef>,      // kind, point, and the name the store gave the blob
}
pub struct PassStart { pub operation: OperationName, pub mode: Mode, pub reason: ChoiceReason, pub driver: Digest,
                       pub queue: Duration, pub prepare: Duration }
pub enum ChoiceReason { FirstInOrder, EarlierDidNotQualify, Pinned, FellBack }
pub struct Locating { pub locator: String, pub position: u32, pub score: Option<u32> }
pub struct Acting { pub means: Vec<Detail>, pub measured: Vec<Detail> }
pub struct Detail { pub name: String, pub value: String }
pub struct TracedValue { pub field: String, pub sensitivity: Sensitivity, pub volatile: bool,
                         pub intended: Option<Value>, pub read_back: Option<Value>,
                         pub normalized_intended: Option<Value>, pub normalized_read_back: Option<Value>,
                         pub matched: Option<bool> }

pub struct OfferedArtifact { pub kind: ArtifactKind, pub at: CapturePoint, pub bytes: Vec<u8>, pub masked: Option<MaskReport> }
pub enum ArtifactKind { Snapshot, Exchange, Screenshot, WindowTree }
pub enum CapturePoint { Step, Checkpoint, Failure }
pub struct MaskReport { pub fields: Vec<String> }
pub enum ArtifactOutcome { Kept(KeptArtifact), Dropped(DropReason) }
pub enum DropReason { Level, Unmasked, HeldSecret, NotText }

pub struct FinishedTrace { pub schema: u32, pub bytes: Vec<u8>, pub signature: Digest, pub records: u32, pub kept: u32,
                           pub limitations: Vec<Limitation> }
pub struct TraceRefused { pub signature: Digest, pub records: u32, pub limitation: Limitation }   // converts to Failure

/// The schema, as types that read a trace back: for `trace.show`, `trace.export`, and tests.
pub const TRACE_SCHEMA: u32 = 1;
pub enum TraceLine { Header(HeaderLine), Step(Box<StepLine>), End(EndLine) }
pub fn read_trace(bytes: &[u8]) -> Result<Vec<TraceLine>, Failure>;
/// Whether the lines still give the signature their end line states.
pub enum TraceCheck { Whole, Cut, Broken }
pub fn check_trace(lines: &[TraceLine]) -> TraceCheck;

/// Text helpers: tags and hidden characters out; hidden characters out; `&amp;` and `&#38;` written out.
pub fn clean_text(text: &str) -> String;
pub fn strip_hidden(text: &str) -> String;
pub fn decode_references(text: &str) -> String;

/// Headers of an exchange, for every pack that records one.
pub const RECORDED_HEADERS: &[&str];
pub fn record_headers<'a>(headers: impl IntoIterator<Item = (&'a str, &'a str)>, redact: &dyn Redact) -> Vec<RecordedHeader>;
pub struct RecordedHeader { pub name: String, pub value: Option<String> }    // None: present, value masked

/// Free text, as every trace field is cleaned and cut.
pub fn clean_text(text: &str) -> String;
pub fn limit_text(text: &str, limit: usize) -> String;

/// Logs, spans, and metrics. Cheap to clone. The engine holds one in its dependencies.
pub struct Telemetry { /* ... */ }
pub fn start(settings: &TelemetrySettings, redact: Arc<dyn Redact>, logs: &Path) -> Result<Telemetry, Failure>;
pub fn start_with_clock(settings: &TelemetrySettings, redact: Arc<dyn Redact>, logs: &Path, clock: Arc<dyn Clock>) -> Result<Telemetry, Failure>;
pub struct TelemetrySettings { pub log_level: LogLevel, pub log_file_bytes: u64, pub log_files_kept: u32 }
impl Telemetry {
    pub fn in_memory(level: LogLevel, redact: Arc<dyn Redact>, clock: Arc<dyn Clock>) -> Self;   // no file; for tests and benchmarks
    pub fn scoped<T>(&self, work: impl FnOnce() -> T) -> T;      // this thread's log lines come here
    pub fn log_destination(&self) -> LogDestination;            // File(path), StandardError(failure), or Memory
    pub fn log_lines(&self) -> Vec<String>;                     // what a telemetry in memory kept
    pub fn receives_process_logs(&self) -> bool;
    pub fn clock(&self) -> Arc<dyn Clock>;

    pub fn span(&self, name: &'static str, parent: Option<&TraceContext>, attrs: &[Attr]) -> SpanGuard;
    pub fn call_span(&self, method: McpMethod, tool: Option<&OperationName>, parent: Option<&TraceContext>) -> SpanGuard;
    pub fn step_span(&self, kind: &StepKind, parent: Option<&TraceContext>, attrs: &[Attr]) -> SpanGuard;
    pub fn count(&self, name: &'static str, attrs: &[Attr], by: u64);
    pub fn observe(&self, name: &'static str, attrs: &[Attr], took: Duration);
    pub fn record_call(&self, tool: &OperationName, plan: &Digest, outcome: CallOutcome, took: Duration);
    pub fn record_retry(&self, tool: &OperationName, plan: &Digest);
    pub fn spans(&self) -> Vec<FinishedSpan>;
    pub fn metrics(&self) -> Metrics;
}
impl SpanGuard { pub fn set(&mut self, attr: Attr); pub fn context(&self) -> TraceContext; }   // ends when dropped

/// Attributes come only from this closed set: none takes free text.
pub enum Attr { Method(McpMethod), Tool(OperationName), Operation(OperationName), Target(TargetName), Plan(Digest),
                Mode(Mode), StepKind(StepKind), Effect(EffectClass), Level(CaptureLevel), Class(FailureClass),
                Code(FailureCode), Count(u64) }
pub enum CallOutcome { Success, Failed(FailureClass) }

/// Incoming context from a request's metadata. Parsed and checked; anything else is ignored.
pub struct TraceContext { /* trace id and parent span id, in the standard text form */ }
impl TraceContext { pub fn parse(traceparent: &str) -> Option<TraceContext>; }

/// Time, so that a test fixes it.
pub trait Clock: Send + Sync { fn elapsed(&self) -> Duration; fn now(&self) -> Timestamp; }
pub struct SystemClock; pub struct ManualClock;
```

Contracts that the signatures do not show:

- **Redaction happens on entry, not on write.** `record` and `artifact` pass every text through the `Redact` before keeping it, so nothing unredacted is held even in memory longer than the call. Text is redacted, cleaned of markup and hidden characters, and redacted again, because cleaning can join the halves of a secret that markup had split. The redactor and the keyed digest are the vault's, given at start; this crate never makes its own.
- **The check on the way out runs once more.** `finish` scans the finished bytes, and every text in them with the JSON escapes undone, with `Redact::contains_secret`. A hit refuses the whole trace: `finish` returns `TraceRefused`, which carries the signature and a `redacted` limitation and no bytes, and an alert is logged. The piece is never handed to the ledger.
- **A text artifact is checked in four forms, and dropped on a hit in any** ([decision 0152](../../decisions/0152-the-action-registry-and-telemetry-after-their-safety-audit.md)). The bytes that are kept are the redacted text with its markup, because a snapshot without its markup is no snapshot. So the redactor is asked about the text as it is kept, about the same text with tags and hidden characters taken out, and about both with their character references written out (`&amp;`, `&#38;`, `&#x26;`). A secret that a tag or a zero-width character splits, or that is written with a reference in a value, is found in one of those forms and the artifact is dropped, with a `redacted` limitation. A record's free text that holds a secret behind a reference is replaced whole by the redaction mark. What this crate relies on from the redactor: that `contains_secret` finds a registered secret in its own text and in the encoded forms the vault knows, wherever it stands in the text handed to it. What it does not find: a secret broken across two elements with white space between them, which reads as two words to a person as well.
- **A debug print shows no value a driver handed over.** `StepRecord`, `TracedValue`, and `Detail` print which parts are present, never a record key, a field's name, a value, or a detail: a step record reaches the trace before anything in it was redacted.
- **Headers and cookies.** `RECORDED_HEADERS` lists the twenty headers whose values may be kept. Every other header is recorded as present with its value masked, whatever the level. Cookies, authorization, and headers that carry an address are not on the list. A pack that builds an exchange calls `record_headers`.
- **Labelled fields.** A value labelled `public` is kept as it is. One labelled `internal` is kept as it is at `standard` and above, and as a keyed digest at `minimal`. One labelled `restricted` is kept only as a keyed digest. Free text is cleaned, and cut at 512 characters with a mark.
- **A record key is never written.** The record holds the workspace's keyed digest of `RecordKey::canonical_bytes()`, under the purpose `record_key`, so that it matches the name the store gives the key.
- **Images arrive masked.** A screenshot is kept only with a `MaskReport` from the pack that took it. Without one it is dropped and a limitation recorded. Outside `full`, a screenshot is kept only at a failure. An image cannot be scanned; the report is the pack's word.
- **What a level keeps.** `minimal`: no artifact. `standard`: a snapshot or a window tree at a checkpoint or a failure, and a screenshot at a failure. `full`: everything offered. Dropping by level is the ordinary case and records no limitation.
- **A trace past its size limit is cut, not failed.** Later records are left out of the bytes and still enter the signature. The end line and `FinishedTrace::limitations` say `truncated`.
- **Spans carry names, timings, and outcomes only.** A span's name and a metric's name are `&'static str`; attributes come from `Attr`, which has no constructor for text. A span's parent is given as a `TraceContext`: no current span is kept per thread. MCP spans follow the OpenTelemetry conventions for MCP, whose names are still in development, so their spelling lives in one table, `Attr::key`.
- **Nothing leaves.** Spans and metrics are kept in memory, a buffer of 4,096 finished spans and one entry per series, for `engine.status`, tests, and the benchmark harness. There is no exporter.
- **Logs are not the record of a run.** They say what the engine did as a program. Crates log with the `tracing` macros; a span made with those macros is ignored. A line that still holds a secret after redaction is replaced by a line that says one was dropped. A line's text is redacted, cleared of characters nobody can see (direction overrides, line and paragraph separators, zero-width characters), and redacted again; its `<` and `>` stay, because a log line is not markup.
- **Reading a trace back.** `check_trace` works the chain out again from the step lines' own digests and compares it with the end line: `whole`, `cut` (the trace passed its size limit, so the lines cannot give the signature), or `broken` (a line missing, added, moved, or repeated, or another signature). `trace.show` reports the word and gives no signature for a broken trace. The signature is not keyed, so this finds damage and a careless edit, not a forger who rewrites the end line too: that a stored trace is the one the engine wrote rests on the store, which seals it. A step line's own digest is taken as written, because it covers values a reader holds only as keyed digests.
- **Time is given.** Nothing reads a clock except through `Clock`. No time enters anything that is hashed.

## The trace signature

Decided in [decision 0076](../../decisions/0076-one-trace-record-per-step-and-a-signature-chained-over-record-digests.md).

Each record gives one SHA-256 digest, its `sig`, over these entries in this order. Each entry is written as its name's length, its name, its value's length, and its value; an entry that may be absent carries one more byte that says whether it is there.

1. A fixed first entry that marks the bytes as a trace record.
2. `pass`, `phase`, `step`, `kind`.
3. `record`: the keyed digest of the record key.
4. `operation` and `mode`, when the record starts a pass.
5. `locator` and `position`, when the step located something.
6. The `means`, counted, each as a name and a value.
7. The values, counted. For each: `field`; `volatile`; unless it is volatile, `normalized_intended` and `normalized_read_back`; and `matched`. A public value enters as its canonical bytes, any other as its keyed digest.
8. `outcome`, and for a failure its class and code.

The signature starts from a fixed digest, replaces it with the digest of itself followed by each record's `sig` in order, and closes with the digest of a fixed mark, the last state, and the count of records.

Text enters after redaction and cleaning and before it is cut for size. Left out: every timing; the run, attempt, and workspace; the tool called and the plan's hash; the engine's version, the registry's digest, and the provider chosen; why a binding was chosen; the score; the `measured` details; raw values; artifact references; the capture level; and a volatile field's value.

## The trace schema, version 1

A trace is UTF-8 text, one JSON object per line, each line ended by a line feed. Entries are written in the order below with no spaces. An entry marked optional is left out when it has no value. A digest is 64 lowercase hexadecimal characters. A duration is whole microseconds. A value is written as `a2m-core` writes a `Value`.

The first line:

| Entry | Holds |
| --- | --- |
| `type` | `header` |
| `schema` | `1` |
| `run` | The run's identifier |
| `attempt` | Which attempt, from 1 |
| `operation` | The tool that was called |
| `plan` | The plan's hash |
| `engine` | The engine's version |
| `registry` | The digest of the registry of providers |
| `level` | `minimal`, `standard`, or `full` |

One line per step:

| Entry | Holds |
| --- | --- |
| `type` | `step` |
| `seq` | The record's number, from 0, counting records that were cut |
| `pass` | Which turn of the engine loop, from 0 |
| `phase` | `act`, `confirm`, or `read_back` |
| `step` | The step's position in its list in the plan |
| `kind` | The step kind |
| `record` | Optional. The keyed digest of the record key |
| `begins` | Optional; on the first record of a pass. `operation`, `mode`, `reason` (`first_in_order`, `earlier_did_not_qualify`, `pinned`, `fell_back`), `driver` (the chosen provider's digest), `queue_us`, `prepare_us` |
| `took_us` | How long the step took |
| `locating` | Optional. `locator`, `position`, and optionally `score` |
| `acting` | Optional. `means` and `measured`, each a list of `name` and `value` |
| `values` | Optional. A list; each has `field`, `sensitivity`, optionally `volatile` when true, optionally `intended`, `read_back`, `normalized_intended`, `normalized_read_back`, and optionally `matched`. Each of the four is `{"value": …}` or `{"keyed": digest}` |
| `outcome` | `state` (`verified`, `done`, or `failed`), and for `failed` its `class` and `code` |
| `artifacts` | Optional. A list of `kind` (`snapshot`, `exchange`, `screenshot`, `window_tree`), `at` (`step`, `checkpoint`, `failure`), and `blob` |
| `sig` | The record's digest in the signature |

The last line:

| Entry | Holds |
| --- | --- |
| `type` | `end` |
| `records` | How many steps the run recorded |
| `kept` | How many of them are in this trace |
| `signature` | The trace signature |
| `limitations` | Optional. A list of `code` and `sentence` |

A trace with no end line is incomplete. A change that a reader of version 1 would misread raises `schema`; a new optional entry does not.

## A log line

One JSON object per line: `ts` (milliseconds since the start of 1970 in UTC), `level` (`error`, `warn`, `info`, `debug`, `trace`), `target`, `message`, and optionally `fields`, in the order the caller gave them. The file is `engine.log` in the folder given to `start`. When the next line would take it past the size in the settings it becomes `engine.log.1`, older files move up by one, and the oldest beyond the count kept is deleted. On Linux and macOS a log file is made with mode `0600`, and one found with wider permissions is narrowed when it is opened: a line is redacted, and it still names runs, sessions, and targets. On Windows a file takes the permissions of its folder, which is in the user's own profile.

`recent_lines(logs: &Path, lines: usize) -> Result<Vec<String>, Failure>` gives the last `lines` lines of the log in `logs`, oldest first: from `engine.log`, and before it from `engine.log.1`, `.2`, and on, until enough are found or a numbered file is missing. It opens no file for writing and makes nothing; a folder that is not there is an empty log. An empty line is left out, and so is a last line of the current file with no line feed after it, which is still being written. The command line's `engine logs` reads the log with it, where the engine runs ([decision 0266](../../decisions/0266-the-engines-log-says-each-start-and-stop-and-what-asked-and-engine-logs-reads-it-where-the-engine-runs.md)).

## Used by, and uses

| Caller | Uses |
| --- | --- |
| `a2m-engine` | `TraceBuilder` for every run attempt; `Telemetry` for spans and `record_call`; hands `FinishedTrace` and kept artifacts to `a2m-ledger` |
| `a2m-ledger` | Nothing at run time; `TRACE_SCHEMA`, to store with each trace blob |
| `a2m-provider` | `ArtifactKind`, `CapturePoint`, and `MaskReport` in what a driver returns; `Telemetry` for spans |
| Packs | `record_headers` when they build an exchange; `MaskReport` when they take an image. The engine offers what they return |
| The front doors | `Telemetry::call_span` with the caller's `TraceContext`; the redactor is the vault's, not this crate's |
| `a2m-actions` | `start` at `Kernel::open`; `read_trace` and the line types for `trace.show` and `trace.export`; `LOG_FILE_NAME` for `engine.logs` |
| `anything2mcp` | `recent_lines` and `LOG_FILE_NAME` for `engine logs`, read by the command line |
| Every crate above it | Logging, through the `tracing` macros and the subscriber this crate installs |
| `bench` | `Telemetry::in_memory` with a `ManualClock`, and `metrics` |

It calls only `a2m-core`, and the `Redact` and `KeyedDigest` it was given.

## State

| State | Where | Notes |
| --- | --- | --- |
| A trace being built | Memory, per run attempt: the bytes so far and one digest | Dropped after `finish` returns |
| Logs | `logs/` in the data directory | Rotated by size; redacted on write |
| Spans and metrics | Memory | Never written to disk by this crate |
| The process's log writer | One per process | The last telemetry started |

## Failures

| Class | Code | When |
| --- | --- | --- |
| `busy` | `storage_full` | The log folder cannot be written. Not returned by `start`: the engine keeps running, logs go to standard error, redacted, and `Telemetry::log_destination` carries the failure. It does not pass by itself |
| `input` | `trace_holds_secret` | A finished trace still held a registered secret at the check on the way out. The trace is dropped; `TraceRefused` converts to this |
| `input` | `telemetry_settings_invalid` | The settings given to `start` name a log file size of zero |
| `busy` | `log_unreadable` | `recent_lines` found a file of the log that is there and cannot be read. It does not pass by itself |
| `input` | `schema_invalid` | `read_trace` met a line that is not a line of a step trace |

A trace that passes its size limit is not a failure. It is cut, and the cut is a `truncated` limitation. `export_unavailable` comes with the exporter.

A failure to capture evidence never fails a read. For a write, the engine decides: under `standard`, a checkpoint snapshot that cannot be kept stops the run before the next record, because the evidence the level promises is missing.

## Invariants

| Must always hold | Proven by |
| --- | --- |
| Five runs of the same thing have one signature, and it is the same on every operating system | `five_runs_have_one_signature`, on simulated runs whose identifiers, timings, scores, and sizes differ, against a signature written into the test (hypothesis H-16). Real runs through a driver come with package 1.2's engine slice |
| The signature is blind to the level, the plan's hash, and why a binding was chosen | `the_signature_is_blind_to_what_legitimately_varies` |
| A changed step, value, order, binding, locator, or outcome changes the signature | `a_changed_step_value_or_order_changes_the_signature`, sixteen changes |
| The same input gives the same bytes, in the schema's order | `the_same_input_gives_the_same_bytes`, against a trace written out in the test |
| A planted secret appears in no record, span, metric, or log line | `a_planted_secret_appears_in_no_trace`, `a_secret_split_by_markup_is_still_masked`, `a_planted_secret_appears_in_no_log_line`, and the run in `a_started_telemetry_logs_to_its_file_and_nothing_carries_a_planted_secret`. All against a test redactor; the same tests against the vault's, and against the test application's traffic, are package 2.1's |
| A trace that holds a registered secret is refused, not written | `a_trace_that_still_holds_a_secret_is_refused` |
| An artifact is redacted and checked, or dropped | `an_artifact_is_redacted_and_checked_or_it_is_dropped`, `artifacts_are_kept_as_the_level_says` |
| A text artifact whose secret is split by markup, by a hidden character, or written with a character reference is dropped | `an_artifact_whose_secret_is_split_by_markup_or_written_with_a_reference_is_dropped` |
| A trace with a line missing, added, moved, or repeated reads back as broken; a debug print of a step record shows no value | `a_trace_is_checked_against_its_own_chain` |
| A log file is its owner's alone | `a_log_file_is_its_owners_alone_and_a_wider_one_is_narrowed`, on Linux and macOS |
| No span or metric can carry a value | The `Attr` type and `&'static str` names; the run in `a_started_telemetry…` searches spans and metrics for planted values |
| A restricted value never appears in a trace | `a_value_is_held_as_its_label_allows` |
| A header off the list is recorded without its value | `a_header_off_the_list_is_recorded_as_present_and_masked` |
| Nothing is sent | There is no exporter. `the_crate_depends_on_nothing_that_could_send_anything` holds the outside crates to four. The test against a listening socket comes with the exporter |
| Logs rotate by size, and a folder that cannot be written sends them to standard error | `rotate::tests`, `logs_rotate_by_size`, `a_folder_that_cannot_be_written_sends_logs_to_standard_error` |
| Spans and metrics read back exactly under a fixed clock | `tests/spans.rs` |
| `standard` capture adds no more than 5% to a run, and a record stays near 2 KB | Not measured. Benchmarks in `bench/`, slice 1.7. The simulated records are 0.5 to 1.3 KB |

## Build order

1. Done: `StepRecord`, `TraceBuilder` with `minimal`, `finish`, and the signature. Package 1.2.
2. Done: the JSON Lines schema, published above; `standard` with snapshots at checkpoints and failures. Package 1.2.
3. Done: logs, with rotation; `recent_lines`, the reader of the recent lines ([decision 0266](../../decisions/0266-the-engines-log-says-each-start-and-stop-and-what-asked-and-engine-logs-reads-it-where-the-engine-runs.md)). The crate's codes are one `failure_codes!` table in `codes`.
4. Done against a test redactor: redaction at capture; the check on the way out; headers, labelled fields. Still to do with package 2.1: the same tests with the vault's redactor, and the planted secret in the test application's traffic.
5. Done: spans, metrics, and the `full` level. Not built: export to an outside collector, its cargo feature, its bounded buffer, `export_unavailable`, and a random trace id.
6. Not built: what policy says about a labelled field beyond its label (masked or left out); the benchmarks.

## Open points

- A signature compares within one workspace whenever a run has a record key or a value that is not public, because both enter as keyed digests. A test that expects a signature is portable only if the test sandbox runs under a fixed, public key. The slice that builds the test runner decides.
- Which of a driver's details are `means` and which are `measured` is each pack's choice. The HTTP pack is the first to make it.
- `ChoiceReason` is a closed list of four, written before the engine's choosing was built. The engine slice confirms or changes it; a change is a new schema version only if a reader of version 1 would misread it.
- A trace is held in memory until `finish`. A pipeline of many thousands of rows meets the size limit first. Streaming to the store is the `EvidenceSink` weighed above.
- Log lines are written synchronously under a lock. A writer thread, if the benchmarks ask for one.
- A text artifact is dropped whole when any form of it holds a secret. Keeping it with the element that holds the secret cut out needs a parser of each kind of snapshot, which belongs to the pack that took it.
- A number cannot be masked in place. A trace in which a number is a registered secret is refused whole; whether to withhold only that record's values instead.
- How well a pack's masking of a screenshot works, which this crate cannot check.
- Whether `full` capture is ever allowed on a production write.
