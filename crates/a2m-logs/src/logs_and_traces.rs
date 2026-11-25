//! The handle every crate above this one holds: logs, spans, and metrics.

use std::fmt;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use a2m_core::{Digest, Failure, FailureClass, OperationName, Redact, StepKind, Timestamp};

use crate::attr::{Attr, CallOutcome, McpMethod};
use crate::clock::{Clock, SystemClock};
use crate::codes::LOG_SETTINGS_INVALID;
use crate::collect::{Collector, FinishedSpan, Metrics};
use crate::context::TraceContext;
use crate::log::{self, LogCore, LogDestination, LogLevel};
use crate::rotate::Rotation;

/// Counter: calls, per tool version and outcome.
pub const CALL_COUNT: &str = "a2m.call.count";
/// Histogram: how long calls took, per tool version.
pub const CALL_DURATION: &str = "a2m.call.duration";
/// Counter: retries, per tool version.
pub const CALL_RETRIES: &str = "a2m.call.retries";
/// The name of a step's span.
pub const STEP_SPAN: &str = "a2m.step";

/// What `start` needs to know. The defaults are those of a local machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogSettings {
    /// How much is logged.
    pub log_level: LogLevel,
    /// The size, in bytes, a log file is rotated at.
    pub log_file_bytes: u64,
    /// How many rotated log files are kept beside the current one.
    pub log_files_kept: u32,
}

impl Default for LogSettings {
    fn default() -> Self {
        Self {
            log_level: LogLevel::Info,
            log_file_bytes: 10 * 1024 * 1024,
            log_files_kept: 5,
        }
    }
}

struct Inner {
    clock: Arc<dyn Clock>,
    log: Arc<LogCore>,
    collector: Mutex<Collector>,
    next_id: AtomicU64,
    /// Whether the `tracing` macros of the whole process reach this handle.
    receives_process_logs: bool,
}

/// Logs, spans, and metrics. Cheap to clone; every clone is the same
/// handle.
///
/// Nothing is sent off the machine. Spans and metrics are kept in memory and
/// read back with [`LogsAndTraces::spans`] and [`LogsAndTraces::metrics`]. Export to
/// an outside collector is not built.
#[derive(Clone)]
pub struct LogsAndTraces {
    inner: Arc<Inner>,
}

/// Starts the logs and traces for a process: opens `engine.log` in the folder `logs`,
/// and makes it the destination of every log line the process writes with the
/// `tracing` macros. Started once by the role; a second `start` in the same
/// process takes the process's logs over from the first.
///
/// A log folder that cannot be written is not an error: the engine keeps
/// running, logs go to standard error, redacted, and
/// [`LogsAndTraces::log_destination`] carries the failure.
///
/// # Errors
///
/// Class `input`, code `log_settings_invalid`, when the settings name a
/// log file size of zero.
pub fn start(
    settings: &LogSettings,
    redact: Arc<dyn Redact>,
    logs: &Path,
) -> Result<LogsAndTraces, Failure> {
    start_with_clock(settings, redact, logs, Arc::new(SystemClock::new()))
}

/// As [`start`], with the clock given. For tests and the benchmark harness.
///
/// # Errors
///
/// As [`start`].
pub fn start_with_clock(
    settings: &LogSettings,
    redact: Arc<dyn Redact>,
    logs: &Path,
    clock: Arc<dyn Clock>,
) -> Result<LogsAndTraces, Failure> {
    if settings.log_file_bytes == 0 {
        return Err(Failure::new(
            FailureClass::Input,
            LOG_SETTINGS_INVALID,
            "The size at which a log file is rotated is zero.",
            "Set the log file size to at least one byte.",
        ));
    }
    let rotation = Rotation {
        max_bytes: settings.log_file_bytes,
        keep: settings.log_files_kept,
    };
    let log = Arc::new(LogCore::to_folder(
        logs,
        settings.log_level,
        rotation,
        redact,
        Arc::clone(&clock),
    ));
    let receives_process_logs = log::install_for_process(Arc::clone(&log));
    Ok(LogsAndTraces::with(clock, log, receives_process_logs))
}

impl LogsAndTraces {
    /// A handle that touches no file and is not installed for the
    /// process: its log lines are kept in memory, beside its spans and
    /// metrics. For tests and the benchmark harness. Log lines reach it from
    /// inside [`LogsAndTraces::scoped`].
    #[must_use]
    pub fn in_memory(level: LogLevel, redact: Arc<dyn Redact>, clock: Arc<dyn Clock>) -> Self {
        let log = Arc::new(LogCore::in_memory(level, redact, Arc::clone(&clock)));
        Self::with(clock, log, false)
    }

    fn with(clock: Arc<dyn Clock>, log: Arc<LogCore>, receives_process_logs: bool) -> Self {
        Self {
            inner: Arc::new(Inner {
                clock,
                log,
                collector: Mutex::new(Collector::default()),
                next_id: AtomicU64::new(1),
                receives_process_logs,
            }),
        }
    }

    /// The clock this logs-and-traces handle measures with.
    #[must_use]
    pub fn clock(&self) -> Arc<dyn Clock> {
        Arc::clone(&self.inner.clock)
    }

    /// Where this logs-and-traces handle's log lines go.
    #[must_use]
    pub fn log_destination(&self) -> LogDestination {
        self.inner.log.destination()
    }

    /// Whether log lines written anywhere in the process reach this
    /// handle. False for one made with [`LogsAndTraces::in_memory`], and for
    /// one started in a process where another subscriber of the `tracing`
    /// crate was installed first.
    #[must_use]
    pub fn receives_process_logs(&self) -> bool {
        self.inner.receives_process_logs
    }

    /// The log lines kept in memory, oldest first. Empty when logs go to a
    /// file or to standard error.
    #[must_use]
    pub fn log_lines(&self) -> Vec<String> {
        self.inner.log.memory_lines()
    }

    /// Runs `work` with this logs-and-traces handle receiving the log lines written on
    /// this thread, whatever is installed for the process.
    pub fn scoped<T>(&self, work: impl FnOnce() -> T) -> T {
        log::scoped(Arc::clone(&self.inner.log), work)
    }

    /// Starts a span named `name`. Under `parent` it joins that trace as a
    /// child; without one it starts a trace of its own. The span ends when
    /// the guard is dropped.
    pub fn span(
        &self,
        name: &'static str,
        parent: Option<&TraceContext>,
        attrs: &[Attr],
    ) -> SpanGuard {
        self.start_span(name.to_owned(), parent, attrs.to_vec())
    }

    /// Starts the span of one MCP call, named for the method and the tool as
    /// OpenTelemetry's conventions for MCP ask. `parent` is the trace context
    /// the caller sent, if any.
    pub fn call_span(
        &self,
        method: McpMethod,
        tool: Option<&OperationName>,
        parent: Option<&TraceContext>,
    ) -> SpanGuard {
        let mut attrs = vec![Attr::Method(method)];
        let name = match tool {
            Some(tool) => {
                attrs.push(Attr::Tool(tool.clone()));
                format!("{} {tool}", method.as_str())
            }
            None => method.as_str().to_owned(),
        };
        self.start_span(name, parent, attrs)
    }

    /// Starts the span of one step.
    pub fn step_span(
        &self,
        kind: &StepKind,
        parent: Option<&TraceContext>,
        attrs: &[Attr],
    ) -> SpanGuard {
        let mut all = vec![Attr::StepKind(kind.clone())];
        all.extend_from_slice(attrs);
        self.start_span(STEP_SPAN.to_owned(), parent, all)
    }

    fn start_span(
        &self,
        name: String,
        parent: Option<&TraceContext>,
        attrs: Vec<Attr>,
    ) -> SpanGuard {
        let span_id = self.next_id();
        let started = self.inner.clock.now();
        let trace_id = parent.map_or_else(
            || self.new_trace_id(started, span_id),
            TraceContext::trace_id,
        );
        SpanGuard {
            logs_and_traces: self.clone(),
            name,
            trace_id,
            span_id,
            parent: parent.map(TraceContext::span_id),
            attrs,
            started,
            started_elapsed: self.inner.clock.elapsed(),
        }
    }

    /// Adds `by` to the counter `name` in the series `attrs`.
    pub fn count(&self, name: &'static str, attrs: &[Attr], by: u64) {
        self.collector().count(name, attrs, by);
    }

    /// Records one duration in the histogram `name` in the series `attrs`.
    pub fn observe(&self, name: &'static str, attrs: &[Attr], took: Duration) {
        self.collector().observe(name, attrs, took);
    }

    /// Records one call of a tool version: its duration, and a count by
    /// outcome. A tool version is the tool's name and the hash of its
    /// compiled target.
    pub fn record_call(
        &self,
        tool: &OperationName,
        compiled_target: &Digest,
        outcome: CallOutcome,
        took: Duration,
    ) {
        let version = [
            Attr::Tool(tool.clone()),
            Attr::CompiledTarget(*compiled_target),
        ];
        let mut collector = self.collector();
        collector.observe(CALL_DURATION, &version, took);
        match outcome {
            CallOutcome::Success => collector.count(CALL_COUNT, &version, 1),
            CallOutcome::Failed(class) => {
                let [tool, compiled_target] = version;
                collector.count(CALL_COUNT, &[tool, compiled_target, Attr::Class(class)], 1);
            }
        }
    }

    /// Records one retry within a call of a tool version.
    pub fn record_retry(&self, tool: &OperationName, compiled_target: &Digest) {
        self.count(
            CALL_RETRIES,
            &[
                Attr::Tool(tool.clone()),
                Attr::CompiledTarget(*compiled_target),
            ],
            1,
        );
    }

    /// The finished spans still in the buffer, oldest first.
    #[must_use]
    pub fn spans(&self) -> Vec<FinishedSpan> {
        self.collector().spans()
    }

    /// Every metric as it stands now, in a stable order.
    #[must_use]
    pub fn metrics(&self) -> Metrics {
        self.collector().metrics()
    }

    fn collector(&self) -> MutexGuard<'_, Collector> {
        self.inner
            .collector
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn next_id(&self) -> u64 {
        self.inner.next_id.fetch_add(1, Ordering::SeqCst).max(1)
    }

    /// A trace id for a trace that starts here: the time of day and the id of
    /// its first span. It is unique within the process and never zero. It is
    /// not random; the slice that builds export replaces it with a random
    /// source, which the standard asks of a trace id that leaves the machine.
    fn new_trace_id(&self, started: Timestamp, span_id: u64) -> u128 {
        let millis = u64::try_from(started.as_unix_millis()).unwrap_or(0);
        (u128::from(millis) << 64) | u128::from(span_id)
    }
}

impl fmt::Debug for LogsAndTraces {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LogsAndTraces")
            .field("log_destination", &self.log_destination())
            .field("receives_process_logs", &self.inner.receives_process_logs)
            .finish_non_exhaustive()
    }
}

/// A span that is running. Dropping the guard ends it.
#[must_use = "a span ends when its guard is dropped"]
pub struct SpanGuard {
    logs_and_traces: LogsAndTraces,
    name: String,
    trace_id: u128,
    span_id: u64,
    parent: Option<u64>,
    attrs: Vec<Attr>,
    started: Timestamp,
    started_elapsed: Duration,
}

impl fmt::Debug for SpanGuard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SpanGuard")
            .field("name", &self.name)
            .field("context", &self.context())
            .field("attrs", &self.attrs)
            .finish_non_exhaustive()
    }
}

impl SpanGuard {
    /// Adds an attribute, such as the class of the failure that ended the
    /// work.
    pub fn set(&mut self, attr: Attr) {
        self.attrs.push(attr);
    }

    /// The context a child of this span is started under, and the text to
    /// send onwards when the work continues in another process.
    #[must_use]
    pub fn context(&self) -> TraceContext {
        TraceContext::new(self.trace_id, self.span_id)
    }
}

impl Drop for SpanGuard {
    fn drop(&mut self) {
        let took = self
            .logs_and_traces
            .inner
            .clock
            .elapsed()
            .saturating_sub(self.started_elapsed);
        let span = FinishedSpan {
            name: std::mem::take(&mut self.name),
            trace_id: self.trace_id,
            span_id: self.span_id,
            parent: self.parent,
            attrs: std::mem::take(&mut self.attrs),
            started: self.started,
            took,
        };
        self.logs_and_traces.collector().finish_span(span);
    }
}
