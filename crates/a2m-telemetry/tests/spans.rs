//! Spans and metrics through the public interface: what a crate above this
//! one calls, and what a test or the benchmark harness reads back.

#![allow(
    clippy::unwrap_used,
    reason = "this file holds only tests; clippy.toml allows unwrap in tests, which it tells by the #[test] attribute, and the helpers the tests share do not carry it"
)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use a2m_core::{
    Digest, EffectClass, FailureClass, Mode, NoSecrets, OperationName, StepKind, Timestamp,
};
use a2m_telemetry::{
    Attr, CALL_COUNT, CALL_DURATION, CALL_RETRIES, CallOutcome, CaptureLevel, LogDestination,
    LogLevel, ManualClock, McpMethod, SPAN_BUFFER, STEP_SPAN, Telemetry, TraceContext,
};

fn telemetry() -> (Telemetry, Arc<ManualClock>) {
    let clock = Arc::new(ManualClock::starting_at(Timestamp::from_unix_millis(
        1_700_000_000_000,
    )));
    let telemetry = Telemetry::in_memory(LogLevel::Info, Arc::new(NoSecrets), clock.clone());
    (telemetry, clock)
}

fn tool() -> OperationName {
    OperationName::new("example_sales_order_read").unwrap()
}

#[test]
fn a_call_and_its_steps_are_one_trace_with_exact_timings() {
    let (telemetry, clock) = telemetry();
    let caller =
        TraceContext::parse("00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01").unwrap();

    let mut call = telemetry.call_span(McpMethod::ToolsCall, Some(&tool()), Some(&caller));
    clock.advance(Duration::from_millis(2));
    {
        let _step = telemetry.step_span(
            &StepKind::new("request").unwrap(),
            Some(&call.context()),
            &[Attr::Mode(Mode::new("http").unwrap())],
        );
        clock.advance(Duration::from_millis(5));
    }
    clock.advance(Duration::from_millis(1));
    call.set(Attr::Class(FailureClass::Drift));
    let call_id = call.context().span_id();
    drop(call);

    // Spans are listed in the order they ended: the step, then the call.
    let spans = telemetry.spans();
    assert_eq!(spans.len(), 2);
    let (step, call) = (&spans[0], &spans[1]);

    assert_eq!(call.name, "tools/call example_sales_order_read");
    assert_eq!(call.trace_id, caller.trace_id());
    assert_eq!(call.parent, Some(caller.span_id()));
    assert_eq!(call.took, Duration::from_millis(8));
    assert_eq!(call.started.as_unix_millis(), 1_700_000_000_000);
    assert_eq!(
        call.attrs,
        [
            Attr::Method(McpMethod::ToolsCall),
            Attr::Tool(tool()),
            Attr::Class(FailureClass::Drift),
        ]
    );
    assert_eq!(call.attrs[0].key(), "mcp.method.name");
    assert_eq!(call.attrs[0].value(), "tools/call");
    assert_eq!(call.attrs[1].key(), "gen_ai.tool.name");

    assert_eq!(step.name, STEP_SPAN);
    assert_eq!(step.trace_id, caller.trace_id());
    assert_eq!(step.parent, Some(call_id));
    assert_eq!(step.took, Duration::from_millis(5));
    assert_eq!(step.started.as_unix_millis(), 1_700_000_000_002);
}

#[test]
fn a_span_with_no_parent_starts_its_own_trace() {
    let (telemetry, _clock) = telemetry();
    let first = telemetry.span("a2m.compile", None, &[Attr::Count(3)]);
    let second = telemetry.span("a2m.compile", None, &[]);
    assert_ne!(first.context().trace_id(), 0);
    assert_ne!(first.context().trace_id(), second.context().trace_id());
    assert_ne!(first.context().span_id(), second.context().span_id());
    // The context is the standard text, ready to send onwards.
    let sent = first.context().to_string();
    assert_eq!(TraceContext::parse(&sent), Some(first.context()));
}

#[test]
fn counters_and_histograms_are_read_back_exactly() {
    let (telemetry, _clock) = telemetry();
    let read = [
        Attr::Effect(EffectClass::Read),
        Attr::Level(CaptureLevel::Minimal),
    ];
    let flipped = [
        Attr::Level(CaptureLevel::Minimal),
        Attr::Effect(EffectClass::Read),
    ];
    let write = [Attr::Effect(EffectClass::Write)];

    telemetry.count("a2m.records", &read, 2);
    // The order attributes are given in does not make a second series.
    telemetry.count("a2m.records", &flipped, 3);
    telemetry.count("a2m.records", &write, 1);
    for micros in [80, 100, 101, 7_000, 90_000_000] {
        telemetry.observe("a2m.step.duration", &read, Duration::from_micros(micros));
    }

    let metrics = telemetry.metrics();
    assert_eq!(metrics.counter("a2m.records", &read), 5);
    assert_eq!(metrics.counter("a2m.records", &write), 1);
    assert_eq!(metrics.counter("a2m.records", &[]), 0);
    assert_eq!(metrics.counters.len(), 2);

    let histogram = metrics.histogram("a2m.step.duration", &read).unwrap();
    assert_eq!(histogram.count, 5);
    assert_eq!(histogram.sum, Duration::from_micros(90_007_281));
    assert_eq!(histogram.min, Duration::from_micros(80));
    assert_eq!(histogram.max, Duration::from_secs(90));
    let counts: Vec<u64> = histogram
        .buckets
        .iter()
        .map(|bucket| bucket.count)
        .collect();
    // At or under 100 µs; under 250 µs; under 10 ms; and past the last bound.
    assert_eq!(counts[0], 2);
    assert_eq!(counts[1], 1);
    assert_eq!(counts[6], 1);
    assert_eq!(counts.last(), Some(&1));
    assert_eq!(counts.iter().sum::<u64>(), 5);
    assert_eq!(histogram.buckets[0].up_to, Some(Duration::from_micros(100)));
    assert_eq!(histogram.buckets.last().unwrap().up_to, None);

    // The same calls give the same reading: nothing depends on the order of
    // a map or on a clock.
    assert_eq!(telemetry.metrics(), metrics);
}

#[test]
fn calls_are_measured_per_tool_version() {
    let (telemetry, _clock) = telemetry();
    let (first, second) = (Digest::of(b"plan 1"), Digest::of(b"plan 2"));
    telemetry.record_call(
        &tool(),
        &first,
        CallOutcome::Success,
        Duration::from_millis(4),
    );
    telemetry.record_call(
        &tool(),
        &first,
        CallOutcome::Success,
        Duration::from_millis(6),
    );
    telemetry.record_call(
        &tool(),
        &first,
        CallOutcome::Failed(FailureClass::Drift),
        Duration::from_millis(9),
    );
    telemetry.record_call(
        &tool(),
        &second,
        CallOutcome::Success,
        Duration::from_millis(1),
    );
    telemetry.record_retry(&tool(), &first);

    let metrics = telemetry.metrics();
    let version =
        |compiled_target: Digest| [Attr::Tool(tool()), Attr::CompiledTarget(compiled_target)];
    assert_eq!(metrics.counter(CALL_COUNT, &version(first)), 2);
    assert_eq!(metrics.counter(CALL_COUNT, &version(second)), 1);
    let drifted = [
        Attr::Tool(tool()),
        Attr::CompiledTarget(first),
        Attr::Class(FailureClass::Drift),
    ];
    assert_eq!(metrics.counter(CALL_COUNT, &drifted), 1);
    assert_eq!(metrics.counter(CALL_RETRIES, &version(first)), 1);
    let durations = metrics.histogram(CALL_DURATION, &version(first)).unwrap();
    assert_eq!(durations.count, 3);
    assert_eq!(durations.sum, Duration::from_millis(19));
}

#[test]
fn the_span_buffer_is_bounded() {
    let (telemetry, _clock) = telemetry();
    for _ in 0..SPAN_BUFFER + 10 {
        drop(telemetry.span("a2m.tick", None, &[]));
    }
    assert_eq!(telemetry.spans().len(), SPAN_BUFFER);
    assert_eq!(telemetry.metrics().spans_dropped, 10);
}

#[test]
fn an_unknown_method_is_not_kept_by_name() {
    let (telemetry, _clock) = telemetry();
    assert_eq!(McpMethod::from_name("tools/call"), McpMethod::ToolsCall);
    let method = McpMethod::from_name("secret/hunter2");
    assert_eq!(method, McpMethod::Other);
    drop(telemetry.call_span(method, None, None));
    assert_eq!(telemetry.spans()[0].name, "other");
}

#[test]
fn a_telemetry_in_memory_touches_no_file_and_is_not_the_process_logger() {
    let (telemetry, _clock) = telemetry();
    assert_eq!(telemetry.log_destination(), LogDestination::Memory);
    assert!(!telemetry.receives_process_logs());
    tracing::info!("written outside the scope, so not received");
    telemetry.scoped(|| tracing::info!(records = 2_u64, "received"));
    let lines = telemetry.log_lines();
    assert_eq!(
        lines,
        [
            "{\"ts\":1700000000000,\"level\":\"info\",\"target\":\"spans\",\"message\":\"received\",\"fields\":{\"records\":2}}"
        ]
    );
}
