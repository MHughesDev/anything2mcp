//! Starting telemetry for a process. `start` installs the process's logger,
//! which can be done once, so everything that needs it is one test.

#![allow(
    clippy::unwrap_used,
    reason = "this file holds only tests; clippy.toml allows unwrap in tests, which it tells by the #[test] attribute, and the helpers the tests share do not carry it"
)]

mod common;

use std::fs;
use std::sync::Arc;
use std::time::Duration;

use a2m_core::{
    Decimal, Digest, FailureClass, OperationName, RecordKey, RunId, Sensitivity, StepKind,
    Timestamp, Value, Version, WorkspaceId,
};
use a2m_telemetry::{
    Attr, CaptureLevel, LOG_FILE_NAME, LogDestination, LogLevel, ManualClock, McpMethod, Phase,
    StepRecord, TelemetrySettings, TraceBuilder, TraceIdentity, TraceOutcome, TracedValue, start,
    start_with_clock,
};

use common::{Planted, TempFolder, TestKey};

const SECRET: &str = "hunter2-session-token";
const VALUE: &str = "a value from the system 4711";

#[test]
fn a_started_telemetry_logs_to_its_file_and_nothing_carries_a_planted_secret() {
    let folder = TempFolder::new("start");
    let logs = folder.path().join("logs");
    let clock = Arc::new(ManualClock::starting_at(Timestamp::from_unix_millis(
        1_700_000_000_000,
    )));
    let redact = Arc::new(Planted::secrets(&[SECRET]));
    let settings = TelemetrySettings {
        log_level: LogLevel::Debug,
        log_file_bytes: 600,
        log_files_kept: 3,
    };

    // A size of zero is refused before anything is touched.
    let zero = TelemetrySettings {
        log_file_bytes: 0,
        ..settings
    };
    let failure = start(&zero, redact.clone(), &logs).err().unwrap();
    assert_eq!(failure.class(), FailureClass::Input);
    assert_eq!(failure.code().as_str(), "telemetry_settings_invalid");
    assert!(!logs.exists());

    let telemetry = start_with_clock(&settings, redact.clone(), &logs, clock.clone()).unwrap();
    let path = logs.join(LOG_FILE_NAME);
    assert_eq!(
        telemetry.log_destination(),
        LogDestination::File(path.clone())
    );
    assert!(telemetry.receives_process_logs());

    // A simulated run: a call span, a step span, a metric, a trace, and log
    // lines from this thread and another, all handed the planted secret and
    // a value a system returned.
    let tool = OperationName::new("example_sales_order_read").unwrap();
    let kind = StepKind::new("request").unwrap();
    let plan = Digest::of(b"the plan");
    let mut trace = TraceBuilder::new(
        TraceIdentity {
            workspace: WorkspaceId::from_bytes([7; 16]),
            run: RunId::from_bytes([1; 16]),
            attempt: 1,
            operation: tool.clone(),
            plan,
            engine: Version::new(0, 1, 0),
            registry: Digest::of(b"registry"),
        },
        CaptureLevel::Full,
        redact.clone(),
        Arc::new(TestKey(b"a key for tests only")),
    );
    {
        let call = telemetry.call_span(McpMethod::ToolsCall, Some(&tool), None);
        let _step = telemetry.step_span(&kind, Some(&call.context()), &[Attr::Count(1)]);
        clock.advance(Duration::from_millis(3));
        tracing::info!(cookie = SECRET, "signed in with {SECRET}");
        tracing::debug!(seen = VALUE, "a detail");
        trace.record(StepRecord {
            pass: 0,
            phase: Phase::Act,
            step: 0,
            kind: kind.clone(),
            record: Some(RecordKey::new(vec![Value::from(VALUE)]).unwrap()),
            begins: None,
            took: Duration::from_millis(3),
            locating: None,
            acting: None,
            values: vec![
                TracedValue::read(
                    "token",
                    Sensitivity::Public,
                    Value::from(SECRET),
                    Value::from(SECRET),
                ),
                TracedValue::read(
                    "customer",
                    Sensitivity::Restricted,
                    Value::from(VALUE),
                    Value::Decimal(Decimal::parse("4711").unwrap()),
                ),
            ],
            outcome: TraceOutcome::Done,
            artifacts: Vec::new(),
        });
    }
    telemetry.record_call(
        &tool,
        &plan,
        a2m_telemetry::CallOutcome::Success,
        Duration::from_millis(3),
    );
    std::thread::spawn(|| tracing::warn!("from another thread, with {SECRET}"))
        .join()
        .unwrap();
    for number in 0..8_u64 {
        tracing::info!(number, "enough lines to rotate the file at least once");
    }
    let trace = trace.finish().unwrap();

    // The planted secret is in no trace, span, metric, or log line. The
    // restricted value is in no trace, and no span or metric has any value.
    let spans = format!("{:?}", telemetry.spans());
    let metrics = format!("{:?}", telemetry.metrics());
    let trace_text = String::from_utf8(trace.bytes).unwrap();
    let mut log_text = String::new();
    let mut log_files = 0;
    for entry in fs::read_dir(&logs).unwrap() {
        let entry = entry.unwrap();
        let content = fs::read_to_string(entry.path()).unwrap();
        assert!(content.len() <= 600, "{}", entry.path().display());
        log_text.push_str(&content);
        log_files += 1;
    }
    for (what, text) in [
        ("spans", &spans),
        ("metrics", &metrics),
        ("trace", &trace_text),
        ("logs", &log_text),
    ] {
        assert!(!text.contains(SECRET), "{what}");
        assert!(!text.contains("hunter2"), "{what}");
    }
    for (what, text) in [
        ("spans", &spans),
        ("metrics", &metrics),
        ("trace", &trace_text),
    ] {
        assert!(!text.contains(VALUE), "{what}");
        assert!(!text.contains("4711"), "{what}");
    }
    assert_eq!(telemetry.spans().len(), 2);

    // The log is one JSON object per line, from every thread, rotated.
    assert!((2..=4).contains(&log_files), "{log_files}");
    assert!(log_text.contains("signed in with [redacted]"));
    assert!(log_text.contains("\"cookie\":\"[redacted]\""));
    assert!(log_text.contains("from another thread, with [redacted]"));
    assert!(log_text.contains("\"level\":\"debug\""));
    for line in log_text.lines() {
        let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
        assert!(
            parsed["ts"].is_i64() && parsed["level"].is_string(),
            "{line}"
        );
    }
    assert!(
        fs::read_to_string(&path)
            .unwrap()
            .ends_with("\"fields\":{\"number\":7}}\n")
    );

    // A second start takes the process's logs over from the first.
    let second_logs = folder.path().join("second");
    let second = start(&TelemetrySettings::default(), redact, &second_logs).unwrap();
    assert!(second.receives_process_logs());
    tracing::info!("to the second telemetry");
    let second_text = fs::read_to_string(second_logs.join(LOG_FILE_NAME)).unwrap();
    assert!(second_text.contains("to the second telemetry"));
    assert!(
        !fs::read_to_string(&path)
            .unwrap()
            .contains("to the second telemetry")
    );
}

/// Nothing leaves the machine: there is no exporter, and no outside crate
/// here that could open a connection. The slice that builds export behind a
/// cargo feature replaces this with a test against a listening socket.
#[test]
fn the_crate_depends_on_nothing_that_could_send_anything() {
    let manifest = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")).unwrap();
    let dependencies: Vec<&str> = manifest
        .lines()
        .skip_while(|line| line.trim() != "[dependencies]")
        .skip(1)
        .take_while(|line| !line.trim_start().starts_with('['))
        .filter_map(|line| line.split(['=', '.']).next())
        .map(str::trim)
        .filter(|name| !name.is_empty() && !name.starts_with('#'))
        .collect();
    assert_eq!(dependencies, ["a2m-core", "serde", "serde_json", "tracing"]);
    assert!(!manifest.contains("[features]"));
}
