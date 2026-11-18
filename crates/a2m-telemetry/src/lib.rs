//! Emitting what the engine did: the step trace and its signature, the
//! capture of artifacts at the level a run asks for, the engine's logs, and
//! spans and metrics.
//!
//! Everything this crate emits has passed a [`Redact`](a2m_core::Redact)
//! first, and is checked for registered secrets once more on the way out. It
//! stores nothing durable: a finished trace and a kept artifact are handed
//! back as redacted bytes for the store. Nothing leaves the machine: spans
//! and metrics are kept in memory, and export to an outside collector is not
//! built.
//!
//! The crates this one may depend on are set by the layering table in
//! `docs/reference/technical-reference.md`: `a2m-core`.

mod attr;
mod clock;
pub mod codes;
mod collect;
mod context;
mod headers;
mod log;
mod recent;
mod rotate;
mod telemetry;
#[cfg(test)]
mod testing;
mod text;
mod trace;

pub use attr::{Attr, CallOutcome, McpMethod};
pub use clock::{Clock, ManualClock, SystemClock};
pub use codes::{LOG_UNREADABLE, STORAGE_FULL, TELEMETRY_SETTINGS_INVALID, TRACE_HOLDS_SECRET};
pub use collect::{Bucket, CounterSample, FinishedSpan, HistogramSample, Metrics, SPAN_BUFFER};
pub use context::TraceContext;
pub use headers::{
    HEADER_VALUE_LIMIT, RECORDED_HEADERS, RecordedHeader, may_record_header, record_header,
    record_headers,
};
pub use log::{LOG_FILE_NAME, LOG_MEMORY_LINES, LOG_TEXT_LIMIT, LogDestination, LogLevel};
pub use recent::recent_lines;
pub use telemetry::{
    CALL_COUNT, CALL_DURATION, CALL_RETRIES, STEP_SPAN, SpanGuard, Telemetry, TelemetrySettings,
    start, start_with_clock,
};
pub use text::{
    FREE_TEXT_LIMIT, LABEL_LIMIT, clean_text, decode_references, limit_text, strip_hidden,
};
pub use trace::{
    Acting, ArtifactKind, ArtifactOutcome, ArtifactRef, CaptureLevel, CapturePoint, ChoiceReason,
    DEFAULT_TRACE_LIMIT, Detail, DropReason, EndLine, FinishedTrace, HeaderLine, Held,
    KeptArtifact, Locating, MaskReport, OfferedArtifact, PassLine, PassStart, Phase, StepLine,
    StepRecord, TRACE_SCHEMA, TraceBuilder, TraceCheck, TraceIdentity, TraceLine, TraceOutcome,
    TraceRefused, TracedValue, ValueLine, check_trace, read_trace,
};
