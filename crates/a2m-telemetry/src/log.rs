//! The engine's own logs: one JSON object per line, levelled, redacted, and
//! rotated by size (spec section 217, "Logs").
//!
//! Every crate logs through the macros of the `tracing` crate. This module is
//! the subscriber behind them. It keeps events and ignores the spans those
//! macros can make: a span that is kept is made through
//! [`Telemetry`](crate::Telemetry), whose attributes cannot carry a value.
//!
//! Logs say what the engine did as a program. They are never the record of a
//! run; that is the ledger and the trace.

use std::collections::VecDeque;
use std::fmt;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError, RwLock};

use a2m_core::{Failure, FailureClass, Redact};
use serde::Serialize;
use serde::ser::SerializeMap;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::subscriber::Interest;
use tracing::{Event, Metadata, Subscriber};

use crate::clock::Clock;
use crate::failure::STORAGE_FULL;
use crate::rotate::{RotatingFile, Rotation};
use crate::text::limit_text;

/// The name of the log file inside the folder a caller gives.
pub const LOG_FILE_NAME: &str = "engine.log";

/// The most characters of a log message, or of one field of a log line.
pub const LOG_TEXT_LIMIT: usize = 4096;

/// How many lines a telemetry that keeps its logs in memory holds.
pub const LOG_MEMORY_LINES: usize = 1024;

/// The line written in place of one that still held a registered secret
/// after redaction.
const DROPPED_MESSAGE: &str = "a log line was dropped because it held a registered secret";

/// How much is logged. Each level includes the ones before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum LogLevel {
    /// Something failed.
    Error,
    /// Something is wrong and the work went on.
    Warn,
    /// What the engine did. The default.
    #[default]
    Info,
    /// Detail for finding a fault.
    Debug,
    /// Everything.
    Trace,
}

impl LogLevel {
    /// The level's name, as written in a log line.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
        }
    }

    fn of(level: tracing::Level) -> Self {
        if level == tracing::Level::ERROR {
            Self::Error
        } else if level == tracing::Level::WARN {
            Self::Warn
        } else if level == tracing::Level::INFO {
            Self::Info
        } else if level == tracing::Level::DEBUG {
            Self::Debug
        } else {
            Self::Trace
        }
    }
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where a telemetry's log lines go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogDestination {
    /// The log file, rotated by size.
    File(PathBuf),
    /// Standard error, because the log folder could not be written. The
    /// failure says so; its class is `busy` and its code `storage_full`.
    StandardError(Failure),
    /// A bounded buffer in memory, read with `Telemetry::log_lines`.
    Memory,
}

/// One field of a log line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
enum FieldValue {
    Text(String),
    Signed(i64),
    Unsigned(u64),
    Flag(bool),
}

/// The fields of a log line, written in the order they were given.
struct Fields<'a>(&'a [(&'static str, FieldValue)]);

impl Serialize for Fields<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (name, value) in self.0 {
            map.serialize_entry(name, value)?;
        }
        map.end()
    }
}

/// One log line. The order of the fields here is the order in the file.
#[derive(Serialize)]
struct Line<'a> {
    /// Milliseconds since the start of 1970 in UTC.
    ts: i64,
    level: &'static str,
    target: &'a str,
    message: &'a str,
    #[serde(skip_serializing_if = "fields_are_empty")]
    fields: Fields<'a>,
}

fn fields_are_empty(fields: &Fields<'_>) -> bool {
    fields.0.is_empty()
}

enum Sink {
    File(RotatingFile),
    Writer(Box<dyn Write + Send>),
    Memory(VecDeque<String>),
}

/// The writer behind one telemetry: it redacts, formats, checks, and writes.
pub(crate) struct LogCore {
    level: LogLevel,
    redact: Arc<dyn Redact>,
    clock: Arc<dyn Clock>,
    sink: Mutex<Sink>,
    /// Set when the log file could not be opened or written.
    degraded: Mutex<Option<Failure>>,
}

impl LogCore {
    /// A writer to `engine.log` in `folder`. When the folder cannot be
    /// written the writer falls back to standard error and says so.
    pub(crate) fn to_folder(
        folder: &Path,
        level: LogLevel,
        rotation: Rotation,
        redact: Arc<dyn Redact>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        let path = folder.join(LOG_FILE_NAME);
        let (sink, degraded) = match RotatingFile::open(&path, rotation) {
            Ok(file) => (Sink::File(file), None),
            Err(error) => (
                Sink::Writer(Box::new(io::stderr())),
                Some(storage_full(error.kind())),
            ),
        };
        Self {
            level,
            redact,
            clock,
            sink: Mutex::new(sink),
            degraded: Mutex::new(degraded),
        }
    }

    /// A writer that keeps its lines in memory.
    pub(crate) fn in_memory(
        level: LogLevel,
        redact: Arc<dyn Redact>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            level,
            redact,
            clock,
            sink: Mutex::new(Sink::Memory(VecDeque::new())),
            degraded: Mutex::new(None),
        }
    }

    /// A writer to anything that can be written to. For tests of the
    /// fallback to standard error.
    #[cfg(test)]
    pub(crate) fn to_writer(
        writer: Box<dyn Write + Send>,
        level: LogLevel,
        redact: Arc<dyn Redact>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            level,
            redact,
            clock,
            sink: Mutex::new(Sink::Writer(writer)),
            degraded: Mutex::new(None),
        }
    }

    pub(crate) fn destination(&self) -> LogDestination {
        if let Some(failure) = self.degraded().clone() {
            return LogDestination::StandardError(failure);
        }
        match &*self.sink.lock().unwrap_or_else(PoisonError::into_inner) {
            Sink::File(file) => LogDestination::File(file.path().to_owned()),
            Sink::Writer(_) | Sink::Memory(_) => LogDestination::Memory,
        }
    }

    pub(crate) fn memory_lines(&self) -> Vec<String> {
        match &*self.sink.lock().unwrap_or_else(PoisonError::into_inner) {
            Sink::Memory(lines) => lines.iter().cloned().collect(),
            Sink::File(_) | Sink::Writer(_) => Vec::new(),
        }
    }

    fn degraded(&self) -> std::sync::MutexGuard<'_, Option<Failure>> {
        self.degraded.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn enabled(&self, level: LogLevel) -> bool {
        level <= self.level
    }

    /// Redacts, formats, checks, and writes one line.
    fn write(
        &self,
        level: LogLevel,
        target: &str,
        message: &str,
        fields: &[(&'static str, FieldValue)],
    ) {
        let ts = self.clock.now().as_unix_millis();
        let message = self.safe(message);
        let fields: Vec<_> = fields
            .iter()
            .map(|(name, value)| {
                let value = match value {
                    FieldValue::Text(text) => FieldValue::Text(self.safe(text)),
                    other => other.clone(),
                };
                (*name, value)
            })
            .collect();
        let line = Line {
            ts,
            level: level.as_str(),
            target,
            message: &message,
            fields: Fields(&fields),
        };
        let mut text = serde_json::to_string(&line).unwrap_or_default();
        // The check on the way out. A number field, or a secret registered
        // after the text was redacted, is caught here.
        if text.is_empty() || self.redact.contains_secret(&text) {
            let dropped = Line {
                ts,
                level: LogLevel::Error.as_str(),
                target: env!("CARGO_PKG_NAME"),
                message: DROPPED_MESSAGE,
                fields: Fields(&[]),
            };
            text = serde_json::to_string(&dropped).unwrap_or_default();
        }
        text.push('\n');
        self.emit(&text);
    }

    /// Text made safe to write: redacted, then limited in length.
    fn safe(&self, text: &str) -> String {
        limit_text(&self.redact.redact(text), LOG_TEXT_LIMIT)
    }

    fn emit(&self, line: &str) {
        let mut sink = self.sink.lock().unwrap_or_else(PoisonError::into_inner);
        let failed = match &mut *sink {
            Sink::File(file) => file.write_line(line.as_bytes()).err(),
            Sink::Writer(writer) => writer.write_all(line.as_bytes()).err(),
            Sink::Memory(lines) => {
                if lines.len() == LOG_MEMORY_LINES {
                    lines.pop_front();
                }
                lines.push_back(line.trim_end().to_owned());
                None
            }
        };
        if let Some(error) = failed
            && matches!(&*sink, Sink::File(_))
        {
            // The engine keeps running and logs to standard error. The line
            // was redacted before it reached here.
            *self.degraded() = Some(storage_full(error.kind()));
            let mut fallback = io::stderr();
            // Nothing more can be done if standard error is closed.
            let _ = fallback.write_all(line.as_bytes());
            *sink = Sink::Writer(Box::new(fallback));
        }
    }
}

/// The failure that says the log folder cannot be written. The kind of the
/// error is named; the path is not, because the caller knows it.
fn storage_full(kind: io::ErrorKind) -> Failure {
    Failure::new(
        FailureClass::Busy,
        STORAGE_FULL,
        format!("The log folder cannot be written ({kind}). Logs are going to standard error."),
        "Free space in the data directory or make its logs folder writable, then restart the engine.",
    )
}

/// Collects one event's message and fields.
#[derive(Default)]
struct Collect {
    message: String,
    fields: Vec<(&'static str, FieldValue)>,
}

impl Collect {
    fn text(&mut self, field: &Field, text: String) {
        if field.name() == "message" {
            self.message = text;
        } else {
            self.fields.push((field.name(), FieldValue::Text(text)));
        }
    }
}

impl Visit for Collect {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.text(field, format!("{value:?}"));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.text(field, value.to_owned());
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.fields.push((field.name(), FieldValue::Signed(value)));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.fields
            .push((field.name(), FieldValue::Unsigned(value)));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.fields.push((field.name(), FieldValue::Flag(value)));
    }
}

/// The writer the whole process logs to, once `start` has installed one. The
/// last telemetry started is the one that receives the process's logs.
static PROCESS: RwLock<Option<Arc<LogCore>>> = RwLock::new(None);

/// Whether this crate's subscriber is the process's default.
static INSTALLED: OnceLock<bool> = OnceLock::new();

/// Makes `core` the writer for the whole process. Returns whether log lines
/// made with the `tracing` macros now reach it: they do not when another
/// subscriber was installed in this process before this crate's.
pub(crate) fn install_for_process(core: Arc<LogCore>) -> bool {
    *PROCESS.write().unwrap_or_else(PoisonError::into_inner) = Some(core);
    *INSTALLED.get_or_init(|| {
        tracing::subscriber::set_global_default(LogSubscriber::new(To::Process)).is_ok()
    })
}

/// Runs `work` with `core` as the writer for this thread only.
pub(crate) fn scoped<T>(core: Arc<LogCore>, work: impl FnOnce() -> T) -> T {
    tracing::subscriber::with_default(LogSubscriber::new(To::One(core)), work)
}

enum To {
    One(Arc<LogCore>),
    Process,
}

/// The subscriber behind the `tracing` macros.
struct LogSubscriber {
    to: To,
    next_span: AtomicU64,
    warned_about_spans: AtomicBool,
}

impl LogSubscriber {
    fn new(to: To) -> Self {
        Self {
            to,
            next_span: AtomicU64::new(1),
            warned_about_spans: AtomicBool::new(false),
        }
    }

    fn core(&self) -> Option<Arc<LogCore>> {
        match &self.to {
            To::One(core) => Some(Arc::clone(core)),
            To::Process => PROCESS
                .read()
                .unwrap_or_else(PoisonError::into_inner)
                .clone(),
        }
    }
}

impl Subscriber for LogSubscriber {
    fn register_callsite(&self, _metadata: &'static Metadata<'static>) -> Interest {
        // Asked every time, never cached: the process's writer, and so its
        // level, can change when a new telemetry is started.
        Interest::sometimes()
    }

    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        self.core()
            .is_some_and(|core| core.enabled(LogLevel::of(*metadata.level())))
    }

    fn new_span(&self, _span: &Attributes<'_>) -> Id {
        // A span made with the macros is given an id and otherwise ignored:
        // its fields are free text, which a kept span may not carry. It is
        // said once, at debug level, so that the gap is not silent.
        if !self.warned_about_spans.swap(true, Ordering::SeqCst)
            && let Some(core) = self.core()
            && core.enabled(LogLevel::Debug)
        {
            core.write(
                LogLevel::Debug,
                env!("CARGO_PKG_NAME"),
                "spans made with the tracing macros are not kept; make spans through Telemetry",
                &[],
            );
        }
        let next = self.next_span.fetch_add(1, Ordering::SeqCst);
        // An id is never zero. The counter starts at one and would take
        // centuries to wrap; if it did, the id falls back to one.
        Id::from_u64(next.max(1))
    }

    fn record(&self, _span: &Id, _values: &Record<'_>) {}

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let Some(core) = self.core() else {
            return;
        };
        let metadata = event.metadata();
        let level = LogLevel::of(*metadata.level());
        if !core.enabled(level) {
            return;
        }
        let mut collected = Collect::default();
        event.record(&mut collected);
        core.write(
            level,
            metadata.target(),
            &collected.message,
            &collected.fields,
        );
    }

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}
}

#[cfg(test)]
mod tests {
    use std::io::{self, Write};
    use std::sync::{Arc, Mutex};

    use a2m_core::{FailureClass, Timestamp};

    use super::{LOG_FILE_NAME, LogCore, LogDestination, LogLevel, scoped};
    use crate::clock::ManualClock;
    use crate::rotate::{Rotation, numbered};
    use crate::testing::{Planted, TempFolder};

    const ROTATION: Rotation = Rotation {
        max_bytes: 1 << 20,
        keep: 2,
    };

    fn clock() -> Arc<ManualClock> {
        Arc::new(ManualClock::starting_at(Timestamp::from_unix_millis(
            1_700_000_000_000,
        )))
    }

    #[test]
    fn a_log_line_is_one_json_object_with_its_fields_in_order() {
        let folder = TempFolder::new("log-line");
        let core = Arc::new(LogCore::to_folder(
            folder.path(),
            LogLevel::Info,
            ROTATION,
            Arc::new(Planted::none()),
            clock(),
        ));
        scoped(Arc::clone(&core), || {
            tracing::info!(target: "a2m_engine", records = 3_u64, mode = "http", ok = true, "run finished");
            tracing::debug!("not written at this level");
        });
        let path = folder.path().join(LOG_FILE_NAME);
        assert_eq!(core.destination(), LogDestination::File(path.clone()));
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            "{\"ts\":1700000000000,\"level\":\"info\",\"target\":\"a2m_engine\",\"message\":\"run finished\",\
             \"fields\":{\"records\":3,\"mode\":\"http\",\"ok\":true}}\n"
        );
    }

    #[test]
    fn a_planted_secret_appears_in_no_log_line() {
        let core = Arc::new(LogCore::in_memory(
            LogLevel::Trace,
            Arc::new(Planted::secrets(&["hunter2-token", "424242"])),
            clock(),
        ));
        scoped(Arc::clone(&core), || {
            tracing::warn!(cookie = "sid=hunter2-token", "signed in with hunter2-token");
            tracing::error!(detail = ?Some("hunter2-token"), "failed");
            // A number cannot be redacted in place, so the line is dropped.
            tracing::info!(pin = 424_242_u64, "a number that is a secret");
        });
        let lines = core.memory_lines();
        assert_eq!(lines.len(), 3);
        for line in &lines {
            assert!(!line.contains("hunter2-token"), "{line}");
            assert!(!line.contains("424242"), "{line}");
        }
        assert!(
            lines[0].contains("signed in with [redacted]"),
            "{}",
            lines[0]
        );
        assert!(lines[0].contains("sid=[redacted]"), "{}", lines[0]);
        assert!(lines[2].contains("was dropped"), "{}", lines[2]);
        assert!(lines[2].contains("\"level\":\"error\""), "{}", lines[2]);
    }

    #[test]
    fn logs_rotate_by_size() {
        let folder = TempFolder::new("log-rotate");
        let rotation = Rotation {
            max_bytes: 200,
            keep: 1,
        };
        let core = Arc::new(LogCore::to_folder(
            folder.path(),
            LogLevel::Info,
            rotation,
            Arc::new(Planted::none()),
            clock(),
        ));
        scoped(Arc::clone(&core), || {
            for number in 0..10_u64 {
                tracing::info!(number, "a line of about ninety bytes once it is written");
            }
        });
        let path = folder.path().join(LOG_FILE_NAME);
        let current = std::fs::read_to_string(&path).unwrap();
        let older = std::fs::read_to_string(numbered(&path, 1)).unwrap();
        assert!(current.len() <= 200 && older.len() <= 200);
        assert!(
            current.ends_with("\"fields\":{\"number\":9}}\n"),
            "{current}"
        );
        assert!(older.lines().count() >= 1);
        assert!(!numbered(&path, 2).exists());
    }

    #[test]
    fn a_folder_that_cannot_be_written_sends_logs_to_standard_error() {
        let folder = TempFolder::new("log-blocked");
        // A file where the folder should be: the folder cannot be made.
        let blocked = folder.path().join("logs");
        std::fs::write(&blocked, "not a folder").unwrap();
        let core = LogCore::to_folder(
            &blocked,
            LogLevel::Info,
            ROTATION,
            Arc::new(Planted::none()),
            clock(),
        );
        let LogDestination::StandardError(failure) = core.destination() else {
            panic!("expected the fallback");
        };
        assert_eq!(failure.class(), FailureClass::Busy);
        assert_eq!(failure.code().as_str(), "storage_full");
    }

    /// A writer the test can read back.
    #[derive(Clone, Default)]
    struct Shared(Arc<Mutex<Vec<u8>>>);

    impl Write for Shared {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn what_goes_to_another_writer_is_redacted_too() {
        let shared = Shared::default();
        let core = Arc::new(LogCore::to_writer(
            Box::new(shared.clone()),
            LogLevel::Info,
            Arc::new(Planted::secrets(&["hunter2-token"])),
            clock(),
        ));
        scoped(core, || tracing::info!("token hunter2-token"));
        let written = String::from_utf8(shared.0.lock().unwrap().clone()).unwrap();
        assert!(written.contains("token [redacted]"), "{written}");
        assert!(!written.contains("hunter2"), "{written}");
    }
}
