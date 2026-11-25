//! The step trace through its public interface: the signature, the bytes,
//! redaction at capture, the check on the way out, and the capture rule.
//!
//! The runs here are simulated: a fixed sequence of steps of the kind an
//! engine would record, with timings, identifiers, and measurements that
//! differ from run to run.

#![allow(
    clippy::unwrap_used,
    reason = "this file holds only tests; clippy.toml allows unwrap in tests, which it tells by the #[test] attribute, and the helpers the tests share do not carry it"
)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use a2m_core::{
    Decimal, Digest, Failure, FailureClass, FailureCode, LimitationCode, Mode, NoSecrets,
    OperationName, RecordKey, Redact, RunId, Sensitivity, StepKind, Timestamp, Value, Values,
    Version, WorkspaceId,
};
use a2m_logs::{
    Acting, CaptureKind, CaptureLevel, CaptureOutcome, CapturePoint, CaptureRef, ChoiceReason,
    Detail, DropReason, FREE_TEXT_LIMIT, FinishedTrace, Held, Locating, LogLevel, LogsAndTraces,
    ManualClock, Mark, MaskReport, OfferedCapture, PassStart, Phase, StepLine, StepRecord,
    TRACE_SCHEMA, TimeMarks, TraceBuilder, TraceCheck, TraceIdentity, TraceLine, TraceOutcome,
    TracedValue, check_trace, read_trace,
};

use common::{Planted, TestKey};

/// The signature of the simulated run. It is written here, not computed, so
/// that the same value is demanded on every operating system the tests run
/// on (hypothesis H-16).
const SIGNATURE: &str = "c320daa83f643fec9149d6a1306580f866dd98bf879eb059346d9cecbef274c1";

const KEY: &[u8] = b"a key for tests only";

fn decimal(text: &str) -> Value {
    Value::Decimal(Decimal::parse(text).unwrap())
}

fn kind(name: &str) -> StepKind {
    StepKind::new(name).unwrap()
}

fn order_key() -> RecordKey {
    RecordKey::new(vec![Value::from("SO-1001")]).unwrap()
}

/// What one trace is the trace of. Everything that can differ between two
/// runs of the same thing differs with `run`.
fn identity(run: u8) -> TraceIdentity {
    TraceIdentity {
        workspace: WorkspaceId::from_bytes([7; 16]),
        run: RunId::from_bytes([run; 16]),
        attempt: u32::from(run),
        operation: OperationName::new("example_sales_order_update").unwrap(),
        compiled_target: Digest::of(b"the plan"),
        engine: Version::new(0, 1, u32::from(run)),
        registry: Digest::of(&[run]),
    }
}

fn step(pass: u32, phase: Phase, step: u32, kind_name: &str, run: u8) -> StepRecord {
    StepRecord {
        pass,
        phase,
        step,
        kind: kind(kind_name),
        record: Some(order_key()),
        begins: None,
        took: Duration::from_micros(1_000 + 37 * u64::from(run) + u64::from(step)),
        locating: None,
        acting: None,
        values: Vec::new(),
        outcome: TraceOutcome::Done,
        captures: Vec::new(),
    }
}

fn begins(operation: &str, mode: &str, run: u8) -> PassStart {
    PassStart {
        operation: OperationName::new(operation).unwrap(),
        mode: Mode::new(mode).unwrap(),
        reason: ChoiceReason::FirstInOrder,
        driver: Digest::of(&[run, 1]),
        queue: Duration::from_millis(u64::from(run)),
        prepare: Duration::from_millis(3 + u64::from(run)),
    }
}

/// The steps of one simulated run: a read by a request, then a write that
/// locates a field, sets it, and reads it back.
fn steps(run: u8) -> Vec<StepRecord> {
    let mut read = step(0, Phase::Act, 0, "request", run);
    read.begins = Some(begins("example_sales_order_read", "http", run));
    read.acting = Some(Acting {
        means: vec![
            Detail::new("method", "GET"),
            Detail::new("path", "/orders/{id}"),
        ],
        measured: vec![
            Detail::new("status", "200"),
            Detail::new("size", (2_000 + u32::from(run)).to_string()),
        ],
    });
    read.values = vec![
        TracedValue::read(
            "total",
            Sensitivity::Public,
            Value::from("1,250.00"),
            decimal("1250.00"),
        ),
        TracedValue::read(
            "customer_email",
            Sensitivity::Restricted,
            Value::from("pat@example.test"),
            Value::from("pat@example.test"),
        ),
        TracedValue::read(
            "note",
            Sensitivity::Internal,
            Value::from("call before delivery"),
            Value::from("call before delivery"),
        ),
        TracedValue::read(
            "updated_at",
            Sensitivity::Public,
            Value::Instant(Timestamp::from_unix_millis(
                1_795_000_000_000 + i64::from(run),
            )),
            Value::Instant(Timestamp::from_unix_millis(
                1_795_000_000_000 + i64::from(run),
            )),
        )
        .volatile(),
    ];

    let mut locate = step(1, Phase::Act, 0, "locate", run);
    locate.begins = Some(begins("example_sales_order_update", "ui", run));
    locate.locating = Some(Locating {
        locator: "role=textbox[name=Total]".to_owned(),
        position: 0,
        score: Some(900 + u32::from(run)),
    });

    let mut set = step(1, Phase::Act, 1, "set", run);
    set.acting = Some(Acting {
        means: vec![Detail::new("rung", "fill")],
        measured: Vec::new(),
    });
    set.values = vec![TracedValue {
        field: "total".to_owned(),
        sensitivity: Sensitivity::Public,
        volatile: false,
        intended: Some(decimal("1300")),
        read_back: None,
        normalized_intended: Some(decimal("1300.00")),
        normalized_read_back: None,
        matched: None,
    }];
    set.captures = vec![CaptureRef {
        kind: CaptureKind::Snapshot,
        at: CapturePoint::Checkpoint,
        blob: Digest::of(&[run, 2]),
    }];

    let mut read_back = step(1, Phase::ReadBack, 0, "read", run);
    read_back.values = vec![TracedValue {
        field: "total".to_owned(),
        sensitivity: Sensitivity::Public,
        volatile: false,
        intended: Some(decimal("1300")),
        read_back: Some(Value::from("1,300.00")),
        normalized_intended: Some(decimal("1300.00")),
        normalized_read_back: Some(decimal("1300.00")),
        matched: Some(true),
    }];
    read_back.outcome = TraceOutcome::Verified;

    vec![read, locate, set, read_back]
}

fn builder(run: u8, level: CaptureLevel, redact: Arc<dyn Redact>) -> TraceBuilder {
    TraceBuilder::new(identity(run), level, redact, Arc::new(TestKey(KEY)))
}

fn trace_of(run: u8, level: CaptureLevel, steps: Vec<StepRecord>) -> FinishedTrace {
    let mut builder = builder(run, level, Arc::new(NoSecrets));
    for step in steps {
        builder.record(step);
    }
    builder.finish().unwrap()
}

fn simulated(run: u8) -> FinishedTrace {
    trace_of(run, CaptureLevel::Standard, steps(run))
}

fn text(trace: &FinishedTrace) -> &str {
    std::str::from_utf8(&trace.bytes).unwrap()
}

fn step_lines(trace: &FinishedTrace) -> Vec<StepLine> {
    read_trace(&trace.bytes)
        .unwrap()
        .into_iter()
        .filter_map(|line| match line {
            TraceLine::Step(step) => Some(*step),
            _ => None,
        })
        .collect()
}

#[test]
fn five_runs_have_one_signature() {
    let traces: Vec<FinishedTrace> = (1..=5).map(simulated).collect();
    for trace in &traces {
        assert_eq!(trace.signature.to_string(), SIGNATURE);
        assert_eq!(trace.records, 4);
    }
    // The runs were not the same bytes: their identifiers and timings differ.
    assert_ne!(traces[0].bytes, traces[1].bytes);
}

#[test]
fn the_signature_is_blind_to_what_legitimately_varies() {
    let base = simulated(1);

    for level in [CaptureLevel::Minimal, CaptureLevel::Full] {
        assert_eq!(trace_of(1, level, steps(1)).signature, base.signature);
    }

    // The compiled target's hash is identity, not behaviour. A test in a
    // target's files may expect a signature, and those files are part of the
    // compiled target.
    let mut other_compiled_target = identity(1);
    other_compiled_target.compiled_target = Digest::of(b"another plan");
    other_compiled_target.operation = OperationName::new("example_sales_order_sync").unwrap();
    let mut builder = TraceBuilder::new(
        other_compiled_target,
        CaptureLevel::Standard,
        Arc::new(NoSecrets),
        Arc::new(TestKey(KEY)),
    );
    for step in steps(1) {
        builder.record(step);
    }
    assert_eq!(builder.finish().unwrap().signature, base.signature);

    // Why a binding was chosen explains the choice; the choice is the mode.
    let mut reason = steps(1);
    reason[1].begins.as_mut().unwrap().reason = ChoiceReason::Pinned;
    assert_eq!(
        trace_of(1, CaptureLevel::Standard, reason).signature,
        base.signature
    );
}

#[test]
fn a_changed_step_value_or_order_changes_the_signature() {
    type Edit = fn(&mut Vec<StepRecord>);
    let edits: [(&str, Edit); 16] = [
        ("two steps in the other order", |steps| steps.swap(1, 2)),
        ("a step missing", |steps| {
            steps.pop();
        }),
        ("a step repeated", |steps| steps.push(steps[3].clone())),
        ("another kind of step", |steps| steps[2].kind = kind("type")),
        ("another part of the loop", |steps| {
            steps[3].phase = Phase::Confirm;
        }),
        ("another position in the compiled target", |steps| {
            steps[2].step = 2
        }),
        ("another pass", |steps| steps[3].pass = 2),
        ("another record", |steps| {
            steps[0].record = Some(RecordKey::new(vec![Value::from("SO-1002")]).unwrap());
        }),
        ("another binding", |steps| {
            steps[1].begins.as_mut().unwrap().mode = Mode::new("page_http").unwrap();
        }),
        ("another locator", |steps| {
            steps[1].locating.as_mut().unwrap().locator = "role=textbox[name=Sum]".to_owned();
        }),
        ("the locator at another position", |steps| {
            steps[1].locating.as_mut().unwrap().position = 1;
        }),
        ("another way of setting", |steps| {
            steps[2].acting.as_mut().unwrap().means[0].value = "type".to_owned();
        }),
        ("another normalized value", |steps| {
            steps[3].values[0].normalized_read_back = Some(decimal("1300.01"));
        }),
        ("another restricted value", |steps| {
            steps[0].values[1].normalized_read_back = Some(Value::from("sam@example.test"));
        }),
        ("a value that did not match", |steps| {
            steps[3].values[0].matched = Some(false);
        }),
        ("another outcome", |steps| {
            steps[3].outcome = TraceOutcome::Failed {
                class: FailureClass::Mismatch,
                code: FailureCode::new("read_back_differs").unwrap(),
            };
        }),
    ];

    let mut seen = vec![simulated(1).signature];
    for (what, edit) in edits {
        let mut changed = steps(1);
        edit(&mut changed);
        let signature = trace_of(1, CaptureLevel::Standard, changed).signature;
        assert!(!seen.contains(&signature), "{what}");
        seen.push(signature);
    }

    // The digest of a restricted value is keyed: another installation's key
    // gives another signature, so signatures compare within one.
    let mut other_key = TraceBuilder::new(
        identity(1),
        CaptureLevel::Standard,
        Arc::new(NoSecrets),
        Arc::new(TestKey(b"another key")),
    );
    for step in steps(1) {
        other_key.record(step);
    }
    assert!(!seen.contains(&other_key.finish().unwrap().signature));
}

#[test]
fn the_same_input_gives_the_same_bytes() {
    assert_eq!(simulated(3).bytes, simulated(3).bytes);

    // One small trace, byte for byte: the order of the entries is the schema.
    let mut read = step(0, Phase::Act, 0, "request", 0);
    read.record = None;
    read.begins = Some(begins("example_sales_order_read", "http", 0));
    read.values = vec![TracedValue::read(
        "total",
        Sensitivity::Public,
        Value::from("1,250.00"),
        decimal("1250.00"),
    )];
    let trace = trace_of(0, CaptureLevel::Minimal, vec![read]);
    let zeros = "0".repeat(32);
    let compiled_target = Digest::of(b"the plan");
    let registry = Digest::of(&[0]);
    let driver = Digest::of(&[0, 1]);
    let sig = step_lines(&trace)[0].sig;
    let expected = format!(
        "{{\"type\":\"header\",\"schema\":1,\"run\":\"r_{zeros}\",\"attempt\":0,\
         \"operation\":\"example_sales_order_update\",\"plan\":\"{compiled_target}\",\"engine\":\"0.1.0\",\
         \"registry\":\"{registry}\",\"level\":\"minimal\"}}\n\
         {{\"type\":\"step\",\"seq\":0,\"pass\":0,\"phase\":\"act\",\"step\":0,\"kind\":\"request\",\
         \"begins\":{{\"operation\":\"example_sales_order_read\",\"mode\":\"http\",\
         \"reason\":\"first_in_order\",\"driver\":\"{driver}\",\"queue_us\":0,\"prepare_us\":3000}},\
         \"took_us\":1000,\
         \"values\":[{{\"field\":\"total\",\"sensitivity\":\"public\",\
         \"read_back\":{{\"value\":\"1,250.00\"}},\
         \"normalized_read_back\":{{\"value\":{{\"decimal\":\"1250\"}}}}}}],\
         \"outcome\":{{\"state\":\"done\"}},\"sig\":\"{sig}\"}}\n\
         {{\"type\":\"end\",\"records\":1,\"kept\":1,\"signature\":\"{}\"}}\n",
        trace.signature
    );
    assert_eq!(text(&trace), expected);
    assert_eq!(trace.schema, TRACE_SCHEMA);
}

#[test]
fn a_trace_reads_back_as_its_lines() {
    let trace = simulated(2);
    let lines = read_trace(&trace.bytes).unwrap();
    assert_eq!(lines.len(), 6);
    let TraceLine::Header(header) = &lines[0] else {
        panic!("the first line is the header");
    };
    assert_eq!(header.schema, TRACE_SCHEMA);
    assert_eq!(header.run, RunId::from_bytes([2; 16]));
    assert_eq!(header.level, CaptureLevel::Standard);
    let TraceLine::End(end) = &lines[5] else {
        panic!("the last line is the end");
    };
    assert_eq!((end.records, end.kept), (4, 4));
    assert_eq!(end.signature, trace.signature);
    assert!(end.limitations.is_empty());

    let steps = step_lines(&trace);
    assert_eq!(
        steps.iter().map(|step| step.seq).collect::<Vec<_>>(),
        [0, 1, 2, 3]
    );
    assert_eq!(steps[1].begins.as_ref().unwrap().mode.as_str(), "ui");
    assert_eq!(steps[2].captures.len(), 1);
    assert_eq!(steps[3].outcome, TraceOutcome::Verified);
    // The record's key is never written: only the workspace's digest of it.
    assert!(steps[0].record.is_some());
    assert!(!text(&trace).contains("SO-1001"));

    let failure: Failure = read_trace(b"{\"type\":\"header\"}\nnot json\n").unwrap_err();
    assert_eq!(failure.class(), FailureClass::Input);
    assert_eq!(failure.code(), &FailureCode::SCHEMA_INVALID);
    assert!(failure.message().contains("Line 1"), "{failure}");
}

/// The time marks of an attempt are written in the end line, in the
/// schema's order, read back, and never enter the signature: two runs whose
/// marks differ have one signature, and a trace written before the marks
/// were added still reads, and checks whole (decision 0999).
#[test]
fn the_time_marks_are_in_the_end_line_and_not_in_the_signature() {
    let marked = |run: u8, marks: TimeMarks| {
        let mut builder = builder(run, CaptureLevel::Standard, Arc::new(NoSecrets));
        for step in steps(run) {
            builder.record(step);
        }
        builder.mark(marks);
        builder.finish().unwrap()
    };
    let marks = TimeMarks {
        run_us: 4_200,
        policy: Mark {
            calls: 1,
            took_us: 40,
        },
        scheduling: Mark::default(),
        session: Mark {
            calls: 1,
            took_us: 7,
        },
        ledger: Mark {
            calls: 6,
            took_us: 900,
        },
        loading: Mark {
            calls: 1,
            took_us: 12,
        },
    };
    let trace = marked(1, marks);
    assert_eq!(trace.signature, simulated(1).signature);
    assert_eq!(trace.signature.to_string(), SIGNATURE);
    let other = TimeMarks {
        run_us: 99_000,
        ..TimeMarks::default()
    };
    assert_eq!(marked(1, other).signature, trace.signature);
    assert_eq!(marks.policy_scheduling_and_ledger_us(), 940);

    // The end line, byte for byte: a part with no call is left out.
    let last = text(&trace).lines().last().unwrap().to_owned();
    assert_eq!(
        last,
        format!(
            "{{\"type\":\"end\",\"records\":4,\"kept\":4,\"signature\":\"{}\",\
             \"marks\":{{\"run_us\":4200,\"policy\":{{\"calls\":1,\"took_us\":40}},\
             \"session\":{{\"calls\":1,\"took_us\":7}},\
             \"ledger\":{{\"calls\":6,\"took_us\":900}},\
             \"loading\":{{\"calls\":1,\"took_us\":12}}}}}}",
            trace.signature
        )
    );
    let lines = read_trace(&trace.bytes).unwrap();
    assert_eq!(check_trace(&lines), TraceCheck::Whole);
    let Some(TraceLine::End(end)) = lines.last() else {
        panic!("the last line is the end");
    };
    assert_eq!(end.marks, Some(marks));

    // A trace with no marks, as every trace before them was, reads with none.
    let unmarked = read_trace(&simulated(1).bytes).unwrap();
    let Some(TraceLine::End(end)) = unmarked.last() else {
        panic!("the last line is the end");
    };
    assert_eq!(end.marks, None);
    assert!(!text(&simulated(1)).contains("marks"));
}

#[test]
fn a_value_is_held_as_its_label_allows() {
    for level in [
        CaptureLevel::Minimal,
        CaptureLevel::Standard,
        CaptureLevel::Full,
    ] {
        let trace = trace_of(1, level, steps(1));
        // A restricted value never appears, at any level.
        assert!(!text(&trace).contains("pat@example.test"), "{level:?}");
        let values = &step_lines(&trace)[0].values;
        let held = |field: &str| {
            values
                .iter()
                .find(|value| value.field == field)
                .unwrap()
                .normalized_read_back
                .clone()
                .unwrap()
        };
        assert!(matches!(held("total"), Held::Value(_)));
        assert!(matches!(held("customer_email"), Held::Keyed(_)));
        // An internal value is kept at `standard` and above.
        let note_is_kept = text(&trace).contains("call before delivery");
        assert_eq!(note_is_kept, level != CaptureLevel::Minimal, "{level:?}");
    }

    // Equal restricted values have equal digests, so a comparison still shows.
    let mut same = steps(1);
    same[0].values[1].intended = Some(Value::from("pat@example.test"));
    let trace = trace_of(1, CaptureLevel::Standard, same);
    let email = &step_lines(&trace)[0].values[1];
    assert_eq!(email.intended, email.read_back);
}

#[test]
fn a_planted_secret_appears_in_no_trace() {
    const SECRET: &str = "hunter2-session-token";
    let planted = |secret: &str| {
        let mut steps = steps(1);
        steps[0].values.push(TracedValue::read(
            format!("echo {secret}"),
            Sensitivity::Public,
            Value::Record(
                Values::new()
                    .with("token", secret)
                    .with(secret, vec![Value::from(format!("Bearer {secret}"))]),
            ),
            Value::from(format!("se<b></b>ssion {secret}")),
        ));
        steps[0]
            .acting
            .as_mut()
            .unwrap()
            .measured
            .push(Detail::new("set-cookie", secret));
        steps[0]
            .acting
            .as_mut()
            .unwrap()
            .means
            .push(Detail::new("query", format!("?t={secret}")));
        steps[1].locating.as_mut().unwrap().locator = format!("text={secret}");
        steps
    };

    let redact = Arc::new(Planted::secrets(&[SECRET]));
    let mut builder = builder(1, CaptureLevel::Full, redact);
    for step in planted(SECRET) {
        builder.record(step);
    }
    let trace = builder.finish().unwrap();
    assert!(!text(&trace).contains(SECRET));
    assert!(!text(&trace).contains("hunter2"));
    assert!(text(&trace).contains("[redacted]"));

    // A secret that differs from run to run, such as a session token, does
    // not make two runs differ: it is masked before the signature sees it.
    let other = Arc::new(Planted::secrets(&["another-session-token"]));
    let mut builder = self::builder(2, CaptureLevel::Full, other);
    for step in planted("another-session-token") {
        builder.record(step);
    }
    assert_eq!(builder.finish().unwrap().signature, trace.signature);
}

#[test]
fn a_secret_split_by_markup_is_still_masked() {
    let redact = Arc::new(Planted::secrets(&["hunter2"]));
    let mut builder = builder(1, CaptureLevel::Standard, redact);
    let mut steps = steps(1);
    steps[0].values[0].read_back = Some(Value::from("hun<i>ter2</i> and hun\u{200B}ter2"));
    for step in steps {
        builder.record(step);
    }
    let trace = builder.finish().unwrap();
    assert!(!text(&trace).contains("hunter2"));
}

#[test]
fn a_trace_that_still_holds_a_secret_is_refused() {
    let clock = Arc::new(ManualClock::starting_at(Timestamp::from_unix_millis(0)));
    let redact = Arc::new(Planted::default());
    let logs_and_traces = LogsAndTraces::in_memory(LogLevel::Info, redact.clone(), clock);

    let refused = logs_and_traces.scoped(|| {
        let mut builder = builder(1, CaptureLevel::Standard, redact.clone());
        for step in steps(1) {
            builder.record(step);
        }
        // The secret is registered after text that holds it was recorded.
        redact.register("call before delivery");
        builder.finish().unwrap_err()
    });
    assert_eq!(refused.signature.to_string(), SIGNATURE);
    assert_eq!(refused.records, 4);
    assert_eq!(refused.limitation.code, LimitationCode::Redacted);

    let failure = Failure::from(refused);
    assert_eq!(failure.class(), FailureClass::Input);
    assert_eq!(failure.code().as_str(), "trace_holds_secret");
    assert!(!failure.to_string().contains("call before"));

    // The alert is a log line, and it does not repeat the secret.
    let lines = logs_and_traces.log_lines();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].contains("\"level\":\"error\""), "{}", lines[0]);
    assert!(
        lines[0].contains("held a registered secret"),
        "{}",
        lines[0]
    );
    assert!(!lines[0].contains("call before"), "{}", lines[0]);

    // A number cannot be masked in place. If it is a secret, the trace goes.
    let redact = Arc::new(Planted::secrets(&["31337"]));
    let mut builder = self::builder(1, CaptureLevel::Standard, redact);
    let mut steps = steps(1);
    steps[0].values[0].normalized_read_back = Some(Value::Int(31337));
    for step in steps {
        builder.record(step);
    }
    assert!(builder.finish().is_err());
}

#[test]
fn free_text_is_cleaned_and_cut_and_the_signature_covers_all_of_it() {
    let with_note = |note: String| {
        let mut steps = steps(1);
        steps[0].values[0].read_back = Some(Value::from(note.clone()));
        steps[0].values[0].normalized_read_back = Some(Value::from(note));
        trace_of(1, CaptureLevel::Standard, steps)
    };

    let trace = with_note("<b>paid</b>\u{202E} in <i>full</i>".to_owned());
    assert!(text(&trace).contains("\"paid in full\""));

    let long = "x".repeat(FREE_TEXT_LIMIT + 100);
    let first = with_note(format!("{long}a"));
    let second = with_note(format!("{long}b"));
    let kept = format!("\"{}…\"", "x".repeat(FREE_TEXT_LIMIT));
    assert!(text(&first).contains(&kept));
    // The two traces show the same cut text, and are still two signatures.
    let shown = |trace: &FinishedTrace| step_lines(trace)[0].values[0].clone();
    assert_eq!(shown(&first), shown(&second));
    assert_ne!(first.signature, second.signature);
}

#[test]
fn a_trace_past_its_size_limit_is_cut_and_says_so() {
    let whole = simulated(1);
    let second_line_ends = text(&whole)
        .match_indices('\n')
        .nth(1)
        .map(|(at, _)| at + 1)
        .unwrap();

    let mut builder =
        builder(1, CaptureLevel::Standard, Arc::new(NoSecrets)).with_limit(second_line_ends);
    for step in steps(1) {
        builder.record(step);
    }
    assert!(builder.is_cut());
    let cut = builder.finish().unwrap();

    // The run is not stopped, and its signature is the whole run's.
    assert_eq!(cut.signature, whole.signature);
    assert_eq!((cut.records, cut.kept), (4, 1));
    assert_eq!(cut.limitations.len(), 1);
    assert_eq!(cut.limitations[0].code, LimitationCode::Truncated);
    assert!(cut.limitations[0].sentence.contains("1 of 4"));

    // The kept part is the whole trace's first lines, then an end line.
    assert!(text(&cut).starts_with(&text(&whole)[..second_line_ends]));
    let lines = read_trace(&cut.bytes).unwrap();
    assert_eq!(lines.len(), 3);
    let TraceLine::End(end) = &lines[2] else {
        panic!("a cut trace still ends with its end line");
    };
    assert_eq!(end.limitations, cut.limitations);
}

fn offered(kind: CaptureKind, at: CapturePoint) -> OfferedCapture {
    OfferedCapture {
        kind,
        at,
        bytes: b"- heading \"Order SO-1001\"".to_vec(),
        masked: (kind == CaptureKind::Screenshot).then(MaskReport::default),
    }
}

#[test]
fn captures_are_kept_as_the_level_says() {
    use CaptureKind::{Exchange, Screenshot, Snapshot, WindowTree};
    use CapturePoint::{Checkpoint, Failure, Step};

    let kept = |level: CaptureLevel, kind: CaptureKind, at: CapturePoint| {
        let mut builder = builder(1, level, Arc::new(NoSecrets));
        let outcome = builder.capture(offered(kind, at));
        assert_eq!(
            matches!(outcome, CaptureOutcome::Kept(_)),
            level.keeps(kind, at)
        );
        if let CaptureOutcome::Dropped(reason) = &outcome {
            assert_eq!(*reason, DropReason::Level);
            // Dropping by level is the ordinary case, not a weakness.
            assert!(builder.finish().unwrap().limitations.is_empty());
        }
        matches!(outcome, CaptureOutcome::Kept(_))
    };

    for kind in [Snapshot, Exchange, Screenshot, WindowTree] {
        for at in [Step, Checkpoint, Failure] {
            assert!(!kept(CaptureLevel::Minimal, kind, at));
            assert!(kept(CaptureLevel::Full, kind, at));
        }
    }
    for kind in [Snapshot, WindowTree] {
        assert!(!kept(CaptureLevel::Standard, kind, Step));
        assert!(kept(CaptureLevel::Standard, kind, Checkpoint));
        assert!(kept(CaptureLevel::Standard, kind, Failure));
    }
    // Outside `full`, a screenshot is kept only at a failure.
    assert!(!kept(CaptureLevel::Standard, Screenshot, Step));
    assert!(!kept(CaptureLevel::Standard, Screenshot, Checkpoint));
    assert!(kept(CaptureLevel::Standard, Screenshot, Failure));
    for at in [Step, Checkpoint, Failure] {
        assert!(!kept(CaptureLevel::Standard, Exchange, at));
    }
}

/// A redactor that finds one secret it cannot mask, as a redactor would
/// whose list grew between the two calls.
struct FindsMore;

impl Redact for FindsMore {
    fn redact(&self, text: &str) -> String {
        text.replace("hunter2", "[redacted]")
    }

    fn contains_secret(&self, text: &str) -> bool {
        text.contains("hunter2") || text.contains("late-secret")
    }
}

#[test]
fn a_capture_is_redacted_and_checked_or_it_is_dropped() {
    let mut builder = builder(1, CaptureLevel::Full, Arc::new(FindsMore));

    // Text is redacted.
    let mut snapshot = offered(CaptureKind::Snapshot, CapturePoint::Checkpoint);
    snapshot.bytes = b"- textbox \"Password\": hunter2".to_vec();
    let CaptureOutcome::Kept(kept) = builder.capture(snapshot) else {
        panic!("a snapshot at a checkpoint is kept");
    };
    assert_eq!(kept.bytes, b"- textbox \"Password\": [redacted]");

    // Text that still holds a secret after redaction is dropped.
    let mut exchange = offered(CaptureKind::Exchange, CapturePoint::Step);
    exchange.bytes = b"{\"token\":\"late-secret\"}".to_vec();
    assert_eq!(
        builder.capture(exchange),
        CaptureOutcome::Dropped(DropReason::HeldSecret)
    );

    // An image without a report of what was masked is dropped.
    let mut screenshot = offered(CaptureKind::Screenshot, CapturePoint::Failure);
    screenshot.masked = None;
    assert_eq!(
        builder.capture(screenshot.clone()),
        CaptureOutcome::Dropped(DropReason::Unmasked)
    );
    assert_eq!(
        builder.capture(screenshot),
        CaptureOutcome::Dropped(DropReason::Unmasked)
    );

    // An image with a report is kept as it arrived, with the report cleaned.
    let mut masked = offered(CaptureKind::Screenshot, CapturePoint::Failure);
    masked.bytes = vec![0x89, 0x50, 0x4e, 0x47, 0xff];
    masked.masked = Some(MaskReport {
        fields: vec!["password".to_owned(), "hunter2".to_owned()],
    });
    let CaptureOutcome::Kept(kept) = builder.capture(masked) else {
        panic!("a masked screenshot at a failure is kept");
    };
    assert_eq!(kept.bytes, [0x89, 0x50, 0x4e, 0x47, 0xff]);
    assert_eq!(kept.masked.unwrap().fields, ["password", "[redacted]"]);

    // Something that should be text and is not cannot be checked.
    let mut broken = offered(CaptureKind::WindowTree, CapturePoint::Failure);
    broken.bytes = vec![0xff, 0xfe];
    assert_eq!(
        builder.capture(broken),
        CaptureOutcome::Dropped(DropReason::NotText)
    );

    // Each kind of drop is recorded once as a limitation of the trace.
    let trace = builder.finish().unwrap();
    assert_eq!(trace.limitations.len(), 3);
    assert!(
        trace
            .limitations
            .iter()
            .all(|limitation| limitation.code == LimitationCode::Redacted)
    );
}

/// A redactor that knows one secret by its own text, as the vault's knows a
/// registered secret: it finds it written out, and in no other form.
struct KnowsOne(&'static str);

impl Redact for KnowsOne {
    fn redact(&self, text: &str) -> String {
        text.replace(self.0, "[redacted]")
    }
}

#[test]
fn a_capture_whose_secret_is_split_by_markup_or_written_with_a_reference_is_dropped() {
    const SECRET: &str = "example-key-ab&cd-7781";
    let offer = |text: &str| {
        let mut snapshot = offered(CaptureKind::Snapshot, CapturePoint::Checkpoint);
        snapshot.bytes = text.as_bytes().to_vec();
        snapshot
    };
    let dropped = CaptureOutcome::Dropped(DropReason::HeldSecret);
    let mut builder = builder(1, CaptureLevel::Standard, Arc::new(KnowsOne(SECRET)));

    // Written out, it is redacted and the snapshot is kept.
    let CaptureOutcome::Kept(kept) = builder.capture(offer(&format!("<td>{SECRET}</td>"))) else {
        panic!("a snapshot with a secret written out is kept, redacted");
    };
    assert_eq!(kept.bytes, b"<td>[redacted]</td>");

    // Split by a tag, by a hidden character, written with a reference in a
    // value, and both at once: the redactor sees none of them in the bytes,
    // and each is still the secret for whoever reads the page.
    for held in [
        "<td>example-key-<wbr>ab&cd-7781</td>".to_owned(),
        "<td>example-key-ab&cd\u{200B}-7781</td>".to_owned(),
        "<input value=\"example-key-ab&amp;cd-7781\">".to_owned(),
        "<input value=\"example-key-ab&#38;cd-7781\">".to_owned(),
        "<input value=\"example-key-ab&#x26;cd-7781\">".to_owned(),
        "<b>example-key-</b>ab&amp;cd<i>-7781</i>".to_owned(),
    ] {
        assert_eq!(builder.capture(offer(&held)), dropped, "{held}");
    }

    // Markup and references that hold no secret do not drop a snapshot.
    let plain = "<p>Total &amp; tax: <b>12</b></p>";
    let CaptureOutcome::Kept(kept) = builder.capture(offer(plain)) else {
        panic!("a snapshot with no secret is kept");
    };
    assert_eq!(kept.bytes, plain.as_bytes());

    // A record's free text with the secret behind a reference is not kept.
    let mut step = step(0, Phase::Act, 0, "fill", 1);
    step.acting = Some(Acting {
        means: vec![Detail::new("typed", "example-key-ab&amp;cd-7781")],
        measured: Vec::new(),
    });
    builder.record(step);
    let trace = builder.finish().unwrap();
    let text = String::from_utf8(trace.bytes.clone()).unwrap();
    assert!(!text.contains("ab&amp;cd"), "{text}");
    assert!(text.contains("[redacted]"), "{text}");
}

#[test]
fn a_trace_is_checked_against_its_own_chain() {
    use a2m_logs::{TraceCheck, check_trace};
    let mut builder = builder(1, CaptureLevel::Minimal, Arc::new(NoSecrets));
    for number in 0..4 {
        builder.record(step(0, Phase::Act, number, "fill", 1));
    }
    let trace = builder.finish().unwrap();
    let lines = read_trace(&trace.bytes).unwrap();
    assert_eq!(check_trace(&lines), TraceCheck::Whole);

    // A step line removed, two swapped, one repeated, the end line gone, and
    // an end line that states another signature.
    let mut removed = lines.clone();
    removed.remove(2);
    let mut swapped = lines.clone();
    swapped.swap(1, 2);
    let mut repeated = lines.clone();
    repeated.insert(2, lines[2].clone());
    let mut headless = lines.clone();
    headless.remove(0);
    let mut endless = lines.clone();
    endless.pop();
    let mut other = lines.clone();
    if let Some(TraceLine::End(end)) = other.last_mut() {
        end.signature = Digest::of(b"another signature");
    }
    for broken in [removed, swapped, repeated, headless, endless, other] {
        assert_eq!(check_trace(&broken), TraceCheck::Broken);
    }

    // A debug print of a record a driver handed over shows no value of it.
    let mut record = step(0, Phase::Act, 0, "fill", 1);
    record.values = vec![TracedValue::read(
        "example-field-name",
        Sensitivity::Restricted,
        Value::from("example-token-in-a-value"),
        Value::from("example-token-in-a-value"),
    )];
    record.acting = Some(Acting {
        means: vec![Detail::new("typed", "example-token-in-a-detail")],
        measured: Vec::new(),
    });
    let printed = format!("{record:?}");
    for hidden in ["example-token", "example-field-name", "SO-1001"] {
        assert!(!printed.contains(hidden), "{printed}");
    }
}
