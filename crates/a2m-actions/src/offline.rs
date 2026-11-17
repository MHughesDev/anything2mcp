//! A target's offline tests, run through the engine against a replay of
//! their fixtures' recorded answers (decision 0177).
//!
//! Each test names a tool of the draft plan, an input, a fixture folder,
//! and the result it must give. The tool runs through the engine with
//! every binding handed a [`Replay`] of the fixture in place of its
//! provider's driver, in the sandbox's posture, under a control plane of
//! its own that holds nothing beyond the test: no session is asked for, no
//! run is stored, no person is asked, and no write can be cleared. The
//! result is compared with what the test states, after the output's field
//! type has normalized both.
//!
//! What a test of a read proves: the binding's steps, as the pack lowered
//! them, turn the recorded answers into the tool's result. What it cannot
//! prove: that the system still answers as recorded. A test of a write is
//! not run yet: it needs a ledger of its own for its intent, its save, and
//! its read-back, which this control plane does not hold.

use std::sync::{Arc, Mutex, PoisonError};

use a2m_auth::{ExpirySign, SessionLease};
use a2m_core::{
    Digest, EffectClass, Epoch, Failure, FailureClass, Plan, RunId, TestKind, TestPlan, ToolRuns,
    Values, WorkspaceId,
};
use a2m_engine::{
    BoxFuture, Clearance, Conditions, Control, Engine, EngineEvent, Evidence, GuardQuestion,
    JobKind, JobOutcome, JobSpec, LeaseClock, Parallelism, Preview, SessionWant, ToolRef,
};
use a2m_ledger::{
    Acted, ActedToken, BlobRef, IntentToken, LockHeld, ReadBack, Reconciled, RecordIntent,
    RecordRef, Settled,
};
use a2m_provider::{CheckedPath, EntryKind, Recorded, RecordedAnswer, Replay};
use a2m_telemetry::CaptureLevel;
use serde_json::{Value as Json, json};

use crate::codes;

/// The largest file of a fixture that is read.
const FIXTURE_FILE_BYTES: u64 = 1024 * 1024;

/// How one offline test ended.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TestOutcome {
    /// `passed`, `failed`, or `not_run`.
    pub(crate) state: &'static str,
    /// What the result shows of it.
    pub(crate) detail: Json,
}

impl TestOutcome {
    fn failed(detail: Json) -> Self {
        Self {
            state: "failed",
            detail,
        }
    }
}

/// The control plane of one offline test: the plan, and the result.
/// Everything that would store, lease, or clear something is refused.
struct OfflineControl {
    plan: Arc<Plan>,
    result: Mutex<Option<Values>>,
}

fn nothing_kept(what: &str) -> Failure {
    Failure::new(
        FailureClass::Policy,
        codes::OFFLINE_WRITES_NOTHING,
        format!("An offline test {what}: it holds no ledger and writes nothing."),
        "Nothing to do; a test of a write is not run offline yet.",
    )
}

impl Control for OfflineControl {
    fn plan<'a>(&'a self, _hash: &'a Digest) -> BoxFuture<'a, Result<Arc<Plan>, Failure>> {
        Box::pin(async move { Ok(Arc::clone(&self.plan)) })
    }

    fn decide<'a>(&'a self, question: &'a GuardQuestion) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move {
            if question.writes {
                return Err(nothing_kept("may not write"));
            }
            Ok(())
        })
    }

    fn session_lease<'a>(
        &'a self,
        _want: &'a SessionWant,
    ) -> BoxFuture<'a, Result<SessionLease, Failure>> {
        Box::pin(async move { Err(nothing_kept("uses no session")) })
    }

    fn report_expiry<'a>(
        &'a self,
        _lease: &'a SessionLease,
        _sign: ExpirySign,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async {})
    }

    fn rate<'a>(
        &'a self,
        _surface: &'a a2m_core::SurfaceName,
        want: u32,
    ) -> BoxFuture<'a, Result<u32, Failure>> {
        Box::pin(async move { Ok(want.max(1)) })
    }

    fn store_preview<'a>(&'a self, _preview: &'a Preview) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move { Err(nothing_kept("stores no preview")) })
    }

    fn approval<'a>(&'a self, _diff: &'a Digest) -> BoxFuture<'a, Result<Clearance, Failure>> {
        Box::pin(async move { Ok(Clearance::Waits) })
    }

    fn lock<'a>(&'a self, _record: RecordRef) -> BoxFuture<'a, Result<LockHeld, Failure>> {
        Box::pin(async move { Err(nothing_kept("locks no record")) })
    }

    fn intend<'a>(
        &'a self,
        _intents: Vec<RecordIntent>,
    ) -> BoxFuture<'a, Result<Vec<IntentToken>, Failure>> {
        Box::pin(async move { Err(nothing_kept("records no intent")) })
    }

    fn acted<'a>(
        &'a self,
        _token: IntentToken,
        _acted: Acted,
    ) -> BoxFuture<'a, Result<ActedToken, Failure>> {
        Box::pin(async move { Err(nothing_kept("sends no write")) })
    }

    fn verified<'a>(
        &'a self,
        _token: &'a ActedToken,
        _read_back: ReadBack,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move { Err(nothing_kept("verifies no write")) })
    }

    fn failed<'a>(
        &'a self,
        _record: RecordRef,
        _why: &'a Failure,
        _evidence: Option<BlobRef>,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move { Ok(()) })
    }

    fn settle<'a>(
        &'a self,
        _record: RecordRef,
        _seen: Reconciled,
    ) -> BoxFuture<'a, Result<Settled, Failure>> {
        Box::pin(async move { Err(nothing_kept("settles no record")) })
    }

    fn send<'a>(&'a self, event: EngineEvent) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move {
            match event {
                EngineEvent::Result(values) => {
                    *self.result.lock().unwrap_or_else(PoisonError::into_inner) = Some(values);
                    Ok(())
                }
                EngineEvent::BeforeCommit { .. } => Err(nothing_kept("keeps nothing for a save")),
                _ => Ok(()),
            }
        })
    }

    fn evidence<'a>(&'a self, blob: Evidence) -> BoxFuture<'a, Result<BlobRef, Failure>> {
        // Named by its digest, and kept nowhere: a test's evidence is what
        // its result says.
        Box::pin(async move { Ok(BlobRef::from_digest(Digest::of(&blob.bytes))) })
    }
}

/// The recorded answers of a fixture folder, or what is wrong with it.
///
/// A fixture folder holds `fixture.yaml`, which says `scrubbed: true`, and
/// one file per step it answers, named after what the step refers to: the
/// entry of the pack's file, such as a call or an endpoint, or, for a step
/// that refers to none, its kind. The file is `<name>.json`: an object of
/// the values the step observed; `null` for a record that is absent; or a
/// list of those, given in order to the steps of that name.
pub(crate) fn fixture_answers(folder: &CheckedPath) -> Result<Vec<RecordedAnswer>, Json> {
    let wrong = |file: &str, why: &str| json!({"file": file, "problem": why});
    if !folder.is_dir() {
        return Err(wrong("", "the fixture folder is not there"));
    }
    let marker = folder
        .join("fixture.yaml")
        .and_then(|file| file.read(FIXTURE_FILE_BYTES))
        .map_err(|_| wrong("fixture.yaml", "the folder has no readable `fixture.yaml`"))?;
    let marker = String::from_utf8_lossy(&marker);
    if !marker
        .lines()
        .any(|line| line.trim().replace(' ', "") == "scrubbed:true")
    {
        return Err(wrong(
            "fixture.yaml",
            "it does not say `scrubbed: true`, so the fixture is not used",
        ));
    }
    let (entries, _) = folder
        .list()
        .map_err(|_| wrong("", "the fixture folder cannot be listed"))?;
    let mut answers = Vec::new();
    for entry in entries {
        let file = entry
            .relative()
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .to_owned();
        if file == "fixture.yaml" || !matches!(entry.entry_kind(), Ok(EntryKind::File)) {
            continue;
        }
        let Some(name) = file.strip_suffix(".json") else {
            // Another kind of recording: a file a step would read, not an
            // answer a step gives.
            answers.push(RecordedAnswer {
                name: format!("\u{0}{file}"),
                args: None,
                answer: Recorded::Absent,
            });
            continue;
        };
        let bytes = entry
            .read(FIXTURE_FILE_BYTES)
            .map_err(|_| wrong(&file, "the file cannot be read"))?;
        let json: Json =
            serde_json::from_slice(&bytes).map_err(|_| wrong(&file, "the file is not JSON"))?;
        let items = match json {
            Json::Array(items) => items,
            one => vec![one],
        };
        for item in items {
            let answer = match item {
                Json::Null => Recorded::Absent,
                Json::Object(_) => Recorded::Done(serde_json::from_value::<Values>(item).map_err(
                    |_| {
                        wrong(
                            &file,
                            "an answer holds a value that cannot be taken: write an exact number as text",
                        )
                    },
                )?),
                _ => {
                    return Err(wrong(
                        &file,
                        "an answer is neither an object of values nor `null`",
                    ));
                }
            };
            answers.push(RecordedAnswer {
                name: name.to_owned(),
                args: None,
                answer,
            });
        }
    }
    Ok(answers)
}

/// Whether a result value is the expected one: the same JSON, or the same
/// text, or the same exact number.
fn same(expected: &Json, got: &Json) -> bool {
    if expected == got {
        return true;
    }
    let text = |value: &Json| match value {
        Json::String(text) => Some(text.clone()),
        Json::Number(number) => Some(number.to_string()),
        Json::Bool(flag) => Some(flag.to_string()),
        _ => None,
    };
    match (text(expected), text(got)) {
        (Some(expected), Some(got)) if expected == got => true,
        (Some(expected), Some(got)) => {
            match (
                expected.parse::<a2m_core::Decimal>(),
                got.parse::<a2m_core::Decimal>(),
            ) {
                (Ok(expected), Ok(got)) => expected == got,
                _ => false,
            }
        }
        _ => false,
    }
}

/// Every difference between what a test expects and the result.
fn differences(expected: &Json, got: &Json) -> Vec<Json> {
    let Json::Object(wanted) = expected else {
        return vec![json!({"field": "", "expected": expected, "got": got})];
    };
    wanted
        .iter()
        .filter(|(field, value)| !got.get(field.as_str()).is_some_and(|found| same(value, found)))
        .map(|(field, value)| {
            json!({"field": field, "expected": value, "got": got.get(field.as_str())})
        })
        .collect()
}

/// Whether the tool a test runs writes.
fn writes(plan: &Plan, test: &TestPlan) -> bool {
    let Some(tool) = plan.tools.get(test.tool.get()) else {
        return false;
    };
    let effect = match tool.runs {
        ToolRuns::Operation(index) => plan.operation(index).map(|found| found.effect),
        ToolRuns::Pipeline(index) => plan.pipeline(index).map(|found| found.effect),
    };
    effect.is_none_or(|effect| effect != EffectClass::Read)
}

/// Runs one offline test of the draft plan through the engine, with the
/// recorded answers of its fixture as [`fixture_answers`] read them.
pub(crate) async fn run_test(
    engine: &Engine,
    plan: &Arc<Plan>,
    plan_hash: Digest,
    workspace: WorkspaceId,
    test: &TestPlan,
    fixture: Option<Result<Vec<RecordedAnswer>, Json>>,
) -> TestOutcome {
    let Some(tool) = plan.tools.get(test.tool.get()) else {
        return TestOutcome::failed(
            json!({"reason": "The test names a tool the plan does not hold."}),
        );
    };
    if test.kind != TestKind::Offline {
        return TestOutcome {
            state: "not_run",
            detail: json!({"reason": "A live test runs against a test record of a system, and is not run here."}),
        };
    }
    if writes(plan, test) {
        return TestOutcome {
            state: "not_run",
            detail: json!({"reason": "An offline test of a write needs a ledger of its own for its intent, its save, and its read-back; that is not built yet (decision 0177)."}),
        };
    }
    let answers = match fixture {
        Some(Ok(answers)) => answers,
        Some(Err(wrong)) => {
            return TestOutcome::failed(
                json!({"reason": "The fixture cannot be used.", "fixture_problem": wrong}),
            );
        }
        None => {
            return TestOutcome::failed(json!({"reason": "The offline test names no fixture."}));
        }
    };
    let not_answers: Vec<String> = answers
        .iter()
        .filter_map(|answer| answer.name.strip_prefix('\u{0}').map(str::to_owned))
        .collect();
    let answers: Vec<RecordedAnswer> = answers
        .into_iter()
        .filter(|answer| !answer.name.starts_with('\u{0}'))
        .collect();
    let replay = Replay::new(answers);
    let control = OfflineControl {
        plan: Arc::clone(plan),
        result: Mutex::new(None),
    };
    // A made-up run of the test's own: it is stored nowhere.
    let mut id = [0_u8; 16];
    id.copy_from_slice(&Digest::of(test.name.as_bytes()).as_bytes()[..16]);
    let run = RunId::from_bytes(id);
    let job = JobSpec {
        run,
        workspace,
        attempt: 1,
        epoch: Epoch::new(1),
        kind: JobKind::Try,
        plan: plan_hash,
        tool: ToolRef::Tool(tool.name.clone()),
        environment: None,
        identity: None,
        input: test.inputs.clone(),
        source: None,
        snapshot: None,
        conditions: Conditions::default(),
        pin: None,
        parallelism: Parallelism::default(),
        lease: LeaseClock::unbounded(),
        capture: CaptureLevel::Minimal,
    };
    let outcome = engine
        .run_offline(&job, &control, Arc::clone(&replay) as _)
        .await;
    let asked = replay.asked();
    match outcome {
        JobOutcome::Finished {
            state: a2m_core::RunState::Completed,
            ..
        } => {
            let got = control
                .result
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take()
                .map(|values| crate::render::plain_values(&values))
                .unwrap_or_else(|| json!({}));
            let expected = test.expect.decode::<Json>().unwrap_or(Json::Null);
            let differs = differences(&expected, &got);
            if differs.is_empty() {
                TestOutcome {
                    state: "passed",
                    detail: json!({"steps": asked}),
                }
            } else {
                TestOutcome::failed(json!({
                    "reason": "The result is not the one the test states.",
                    "differences": differs,
                    "steps": asked,
                }))
            }
        }
        other => {
            let mut detail = json!({
                "reason": "The tool did not complete against its fixture.",
                "steps": asked,
            });
            if let Some(failure) = other.failure() {
                detail["failure"] = json!({
                    "class": failure.class().to_string(),
                    "code": failure.code().to_string(),
                    "message": failure.message(),
                });
            }
            if !not_answers.is_empty() {
                detail["not_answers"] = json!(not_answers);
            }
            TestOutcome::failed(detail)
        }
    }
}
