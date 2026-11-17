//! A target's offline tests, run through the engine against a replay of
//! their fixtures' recorded answers (decisions 0177 and 0180).
//!
//! Each test names a tool of the draft plan, an input, a fixture folder,
//! and the result it must give. The tool runs through the engine with
//! every binding handed a [`Replay`] of the fixture in place of its
//! provider's driver, under a control plane of its own that holds nothing
//! beyond the test: no session is asked for, no person is asked, and
//! nothing of the test reaches the installation's store.
//!
//! A test of a read runs in the sandbox's posture, and its control plane
//! stores nothing at all. A test of a write runs as a run of a ledger of its
//! own: a store made by [`StoreConfig::throwaway`], in memory, gone when the
//! test ends, with the same ledger code and the same nine rules as the
//! installation's. Its intent is recorded before the save, its read-back is
//! compared, and its one clearance, [`WriteClearance::OfflineTest`], is one
//! that store alone takes. The control plane's rule allows exactly the
//! test's tool, and nothing else; the save and the read-back are answered
//! by the replay. What the engine sent to each committing step is compared
//! with the request the fixture recorded.
//!
//! What a test proves: the binding's steps, as the pack lowered them, turn
//! the recorded answers into the tool's result, and a write sends the
//! request the fixture recorded and verifies its read-back. What it cannot
//! prove: that the system still answers as recorded, or accepts the save.

use std::sync::{Arc, Mutex, PoisonError};

use a2m_auth::{ExpirySign, SessionLease};
use a2m_core::{
    Digest, EffectClass, Epoch, Failure, FailureClass, Identity, Plan, PrincipalId, RunId,
    RunState, TestKind, TestPlan, ToolRuns, Value, Values, WorkspaceId,
};
use a2m_engine::{
    BoxFuture, Clearance, Conditions, Control, Engine, EngineEvent, Evidence, GuardQuestion,
    JobKind, JobOutcome, JobSpec, LeaseClock, Parallelism, Preview, SessionWant, ToolRef,
};
use a2m_ledger::{
    Acted, ActedToken, BlobRef, Durability, EventKind, Fence, IntentToken, LockHeld, NewBlob,
    NewRun, Opened, ReadBack, Reconciled, RecordIntent, RecordRef, RetentionClass, Settled, Store,
    StoreConfig, ToolKind, WriteClearance,
};
use a2m_provider::{CheckedPath, EntryKind, Recorded, RecordedAnswer, Replay};
use a2m_telemetry::CaptureLevel;
use a2m_vault::{OwnerRef, Vault};
use serde_json::{Value as Json, json};

use crate::codes;

/// The largest file of a fixture that is read.
const FIXTURE_FILE_BYTES: u64 = 1024 * 1024;

/// The largest source file of a fixture that is read: a fixture's recorded
/// file is at most 16 MiB (`a2m-files`, `file_too_large`).
const FIXTURE_SOURCE_BYTES: u64 = 16 * 1024 * 1024;

/// The suffix of a file that records what a committing step was sent.
const REQUEST_SUFFIX: &str = ".request.json";

/// The names `expect` may use for the run itself, where the result holds
/// no field of that name: its end state, how many records were verified or
/// found unchanged, and the code of the failure it stopped with.
const RUN_FACTS: [&str; 3] = ["state", "records_verified", "failure"];

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

/// What an offline test of a write runs in: the installation's vault, to
/// seal the test's own evidence in its own store, and who asked.
#[derive(Clone)]
pub(crate) struct OfflineWorld {
    /// The workspace the test runs for.
    pub(crate) workspace: WorkspaceId,
    /// The caller, as the throwaway run's principal.
    pub(crate) principal: PrincipalId,
    /// The vault, joined to each throwaway store; `None` when it is not
    /// open, and then a write's test cannot run.
    pub(crate) vault: Option<Arc<Vault>>,
}

/// A fixture folder, read: its recorded answers, the requests its
/// committing steps must be sent, and the folder, for a source a test
/// names.
pub(crate) struct Fixture {
    folder: CheckedPath,
    answers: Vec<RecordedAnswer>,
    requests: Vec<(String, Json)>,
    /// Files of another kind: a file a step or a source reads.
    files: Vec<String>,
}

/// The ledger of one offline test of a write: a throwaway store and the run
/// it holds.
struct OfflineLedger {
    store: Store,
    fence: Fence,
    owner: OwnerRef,
}

/// The control plane of one offline test: the plan, the result, and, for a
/// write, the test's own ledger. Everything else that would store, lease,
/// or clear something is refused.
struct OfflineControl {
    plan: Arc<Plan>,
    tool: a2m_core::OperationName,
    workspace: WorkspaceId,
    result: Mutex<Option<Values>>,
    ledger: Option<OfflineLedger>,
}

fn nothing_kept(what: &str) -> Failure {
    Failure::new(
        FailureClass::Policy,
        codes::OFFLINE_WRITES_NOTHING,
        format!("An offline test {what}: it holds no ledger but its own, and reaches no system."),
        "Nothing to do; an offline test runs only its own tool against its fixture.",
    )
}

impl OfflineControl {
    fn ledger(&self, what: &str) -> Result<&OfflineLedger, Failure> {
        self.ledger.as_ref().ok_or_else(|| nothing_kept(what))
    }
}

impl Control for OfflineControl {
    fn plan<'a>(&'a self, _hash: &'a Digest) -> BoxFuture<'a, Result<Arc<Plan>, Failure>> {
        Box::pin(async move { Ok(Arc::clone(&self.plan)) })
    }

    fn decide<'a>(&'a self, question: &'a GuardQuestion) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move {
            // The test's one rule: its own tool, and a write only where the
            // test holds a ledger of its own.
            if question.tool != self.tool {
                return Err(nothing_kept("may run only the tool it tests"));
            }
            if question.writes && self.ledger.is_none() {
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

    fn store_preview<'a>(&'a self, preview: &'a Preview) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move {
            let ledger = self.ledger("stores no preview")?;
            let (fence, preview) = (ledger.fence, preview.clone());
            ledger
                .store
                .write(Durability::Synced, move |tx| {
                    let mut ledger = tx.ledger();
                    ledger.plan_records(&fence, &preview.records)?;
                    ledger.store_diff(&fence, preview.diff, Some(preview.diff_blob))
                })
                .await
        })
    }

    fn approval<'a>(&'a self, _diff: &'a Digest) -> BoxFuture<'a, Result<Clearance, Failure>> {
        Box::pin(async move {
            let Some(ledger) = &self.ledger else {
                return Ok(Clearance::Waits);
            };
            // The test's own clearance, which only its throwaway store
            // takes: the save it clears is answered by the replay.
            let fence = ledger.fence;
            ledger
                .store
                .write(Durability::Synced, move |tx| {
                    tx.ledger()
                        .begin_writes(&fence, WriteClearance::OfflineTest)
                })
                .await?;
            Ok(Clearance::Cleared)
        })
    }

    fn lock<'a>(&'a self, record: RecordRef) -> BoxFuture<'a, Result<LockHeld, Failure>> {
        Box::pin(async move {
            let ledger = self.ledger("locks no record")?;
            let fence = ledger.fence;
            ledger
                .store
                .write(Durability::Synced, move |tx| {
                    tx.ledger().take_lock(&fence, record)
                })
                .await
        })
    }

    fn intend<'a>(
        &'a self,
        intents: Vec<RecordIntent>,
    ) -> BoxFuture<'a, Result<Vec<IntentToken>, Failure>> {
        Box::pin(async move {
            let ledger = self.ledger("records no intent")?;
            ledger.store.store_intents(&ledger.fence, intents).await
        })
    }

    fn acted<'a>(
        &'a self,
        token: IntentToken,
        acted: Acted,
    ) -> BoxFuture<'a, Result<ActedToken, Failure>> {
        Box::pin(async move {
            let ledger = self.ledger("sends no write")?;
            ledger
                .store
                .write(Durability::Synced, move |tx| {
                    tx.ledger().mark_acted(token, acted)
                })
                .await
        })
    }

    fn verified<'a>(
        &'a self,
        token: &'a ActedToken,
        read_back: ReadBack,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move {
            let ledger = self.ledger("verifies no write")?;
            ledger.store.store_verified(token, read_back).await
        })
    }

    fn acknowledged<'a>(
        &'a self,
        token: &'a ActedToken,
        evidence: Option<BlobRef>,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move {
            let ledger = self.ledger("ends no record")?;
            let evidence = evidence.ok_or_else(|| {
                Failure::new(
                    FailureClass::Busy,
                    a2m_engine::codes::ACKNOWLEDGEMENT_NOT_KEPT,
                    "The recorded answer could not be kept, so the record cannot end on it.",
                    "Run the test again.",
                )
            })?;
            ledger.store.store_acknowledged(token, evidence).await
        })
    }

    fn failed<'a>(
        &'a self,
        record: RecordRef,
        why: &'a Failure,
        evidence: Option<BlobRef>,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move {
            let Some(ledger) = &self.ledger else {
                return Ok(());
            };
            let (fence, why) = (ledger.fence, why.clone());
            ledger
                .store
                .write(Durability::Synced, move |tx| {
                    tx.ledger().mark_failed(&fence, record, &why, evidence)
                })
                .await
        })
    }

    fn settle<'a>(
        &'a self,
        record: RecordRef,
        seen: Reconciled,
    ) -> BoxFuture<'a, Result<Settled, Failure>> {
        Box::pin(async move {
            let ledger = self.ledger("settles no record")?;
            let fence = ledger.fence;
            ledger
                .store
                .write(Durability::Synced, move |tx| {
                    tx.ledger().settle(&fence, record, seen)
                })
                .await
        })
    }

    fn send<'a>(&'a self, event: EngineEvent) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move {
            match event {
                EngineEvent::Result(values) => {
                    *self.result.lock().unwrap_or_else(PoisonError::into_inner) = Some(values);
                    Ok(())
                }
                EngineEvent::Moved(to) => {
                    let Some(ledger) = &self.ledger else {
                        return Ok(());
                    };
                    let fence = ledger.fence;
                    ledger
                        .store
                        .write(Durability::Synced, move |tx| {
                            let mut ledger = tx.ledger();
                            if ledger.run(fence.run)?.state == to {
                                return Ok(());
                            }
                            ledger.set_state(&fence, to, None)
                        })
                        .await
                }
                EngineEvent::BeforeCommit { record, outputs } => {
                    let ledger = self.ledger("keeps nothing for a save")?;
                    let fence = ledger.fence;
                    let payload = json!({
                        "before_commit": true,
                        "step": record.step_seq,
                        "record": record.record_seq,
                    });
                    ledger
                        .store
                        .write(Durability::Synced, move |tx| {
                            tx.ledger()
                                .append_event(&fence, EventKind::Need, &payload, Some(outputs))
                                .map(|_| ())
                        })
                        .await
                }
                _ => Ok(()),
            }
        })
    }

    fn evidence<'a>(&'a self, blob: Evidence) -> BoxFuture<'a, Result<BlobRef, Failure>> {
        Box::pin(async move {
            match &self.ledger {
                // Kept in the test's own store, which goes with the test.
                Some(ledger) => {
                    ledger
                        .store
                        .blobs()
                        .put(NewBlob {
                            workspace: self.workspace,
                            owner: ledger.owner.clone(),
                            kind: blob.kind,
                            class: blob.class,
                            plain: blob.bytes,
                        })
                        .await
                }
                // Named by its digest, and kept nowhere: a read's test's
                // evidence is what its result says.
                None => Ok(BlobRef::from_digest(Digest::of(&blob.bytes))),
            }
        })
    }
}

/// Reads one answer file's items: an object of values, `null`, or a list of
/// those.
fn answer_items(file: &str, json: Json) -> Result<Vec<Recorded>, Json> {
    let wrong = |why: &str| json!({"file": file, "problem": why});
    let items = match json {
        Json::Array(items) => items,
        one => vec![one],
    };
    items
        .into_iter()
        .map(|item| match item {
            Json::Null => Ok(Recorded::Absent),
            Json::Object(_) => serde_json::from_value::<Values>(item)
                .map(Recorded::Done)
                .map_err(|_| {
                    wrong("an answer holds a value that cannot be taken: write an exact number as text")
                }),
            _ => Err(wrong("an answer is neither an object of values nor `null`")),
        })
        .collect()
}

/// The fixture of a folder, or what is wrong with it.
///
/// A fixture folder holds `fixture.yaml`, which says `scrubbed: true`, and
/// one file per step it answers, named after what the step refers to: the
/// entry of the pack's file, such as a call or an endpoint, or, for a step
/// that refers to none, its kind. The file is `<name>.json`: an object of
/// the values the step observed, or `null` for a record that is absent,
/// which answers every step of that name; or a list of those, given in
/// order to the steps of that name, each once. A committing
/// step also has `<name>.request.json`: the values it must be sent, an
/// object, or a list of objects in the order the steps are sent. Any other
/// file is one a step or a source reads.
pub(crate) fn read_fixture(folder: CheckedPath) -> Result<Fixture, Json> {
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
    let mut fixture = Fixture {
        folder: folder.clone(),
        answers: Vec::new(),
        requests: Vec::new(),
        files: Vec::new(),
    };
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
        let Some(stem) = file.strip_suffix(".json") else {
            // Another kind of recording: a file a step or a source reads,
            // not an answer a step gives.
            fixture.files.push(file);
            continue;
        };
        let bytes = entry
            .read(FIXTURE_FILE_BYTES)
            .map_err(|_| wrong(&file, "the file cannot be read"))?;
        let json: Json =
            serde_json::from_slice(&bytes).map_err(|_| wrong(&file, "the file is not JSON"))?;
        if let Some(name) = file.strip_suffix(REQUEST_SUFFIX) {
            let items = match json {
                Json::Array(items) => items,
                one => vec![one],
            };
            for item in items {
                if !item.is_object() {
                    return Err(wrong(&file, "a recorded request is not an object of values"));
                }
                fixture.requests.push((name.to_owned(), item));
            }
            continue;
        }
        // One value answers every step of its name; a list answers them in
        // order, each once.
        let repeats = !json.is_array();
        for answer in answer_items(&file, json)? {
            fixture.answers.push(RecordedAnswer {
                name: stem.to_owned(),
                args: None,
                answer,
                repeats,
            });
        }
    }
    Ok(fixture)
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

/// Every difference between what a test expects and what it got: the
/// result's field of that name, or, where the result holds none, the run's
/// own fact of that name ([`RUN_FACTS`]).
fn differences(expected: &Json, got: &Json, facts: &Json) -> Vec<Json> {
    let Json::Object(wanted) = expected else {
        return vec![json!({"field": "", "expected": expected, "got": got})];
    };
    wanted
        .iter()
        .filter_map(|(field, value)| {
            let found = got.get(field.as_str()).or_else(|| {
                RUN_FACTS
                    .contains(&field.as_str())
                    .then(|| facts.get(field.as_str()))
                    .flatten()
            });
            (!found.is_some_and(|found| same(value, found)))
                .then(|| json!({"field": field, "expected": value, "got": found}))
        })
        .collect()
}

/// Every difference between the requests the fixture recorded and what the
/// engine sent, step by step in order: a request sent with no recorded
/// one, a recorded one never sent, and each field that differs.
fn request_differences(recorded: &[(String, Json)], sent: &[(String, Values)]) -> Vec<Json> {
    let mut found = Vec::new();
    let mut names: Vec<&str> = recorded
        .iter()
        .map(|(name, _)| name.as_str())
        .chain(sent.iter().map(|(name, _)| name.as_str()))
        .collect();
    names.sort_unstable();
    names.dedup();
    for name in names {
        let wanted: Vec<&Json> = recorded
            .iter()
            .filter(|(of, _)| of == name)
            .map(|(_, request)| request)
            .collect();
        let given: Vec<Json> = sent
            .iter()
            .filter(|(of, _)| of == name)
            .map(|(_, args)| crate::render::plain_values(args))
            .collect();
        for at in 0..wanted.len().max(given.len()) {
            match (wanted.get(at), given.get(at)) {
                (Some(wanted), Some(given)) => {
                    let mut fields: Vec<&String> = wanted
                        .as_object()
                        .into_iter()
                        .flat_map(|object| object.keys())
                        .chain(given.as_object().into_iter().flat_map(|object| object.keys()))
                        .collect();
                    fields.sort_unstable();
                    fields.dedup();
                    for field in fields {
                        let (expected, got) = (wanted.get(field), given.get(field));
                        let equal = match (expected, got) {
                            (Some(expected), Some(got)) => same(expected, got),
                            _ => false,
                        };
                        if !equal {
                            found.push(json!({
                                "step": name, "sent": at + 1, "field": field,
                                "expected": expected, "got": got,
                            }));
                        }
                    }
                }
                (Some(wanted), None) => found.push(json!({
                    "step": name, "sent": at + 1, "expected": wanted, "got": null,
                    "problem": "the fixture records a request the engine never sent",
                })),
                (None, Some(given)) => found.push(json!({
                    "step": name, "sent": at + 1, "expected": null, "got": given,
                    "problem": format!("the fixture records no request for this save: add `{name}{REQUEST_SUFFIX}`"),
                })),
                (None, None) => {}
            }
        }
    }
    found
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

/// The source a test of a pipeline gives, read from its fixture folder, and
/// the inputs to call with. A pipeline with a source names the file as the
/// input `source`: a name inside the fixture folder, read through the
/// checked paths, so that no test reads a file outside its target.
fn source_of(
    plan: &Plan,
    test: &TestPlan,
    fixture: &Fixture,
) -> Result<(Values, Option<Arc<Vec<u8>>>), Json> {
    let pipeline_source = plan
        .tools
        .get(test.tool.get())
        .and_then(|tool| match tool.runs {
            ToolRuns::Pipeline(index) => plan.pipeline(index),
            ToolRuns::Operation(_) => None,
        })
        .is_some_and(|pipeline| pipeline.source.is_some());
    if !pipeline_source {
        return Ok((test.inputs.clone(), None));
    }
    // The input keeps the file's name, as a call's does; the bytes are the
    // job's source.
    let inputs = test.inputs.clone();
    let Some(Value::Text(name)) = test.inputs.get("source").cloned() else {
        return Err(json!({
            "problem": "The pipeline reads a source, and the test names none: give `source` in its inputs, the name of a file in its fixture folder."
        }));
    };
    let bytes = fixture
        .folder
        .join(&name)
        .and_then(|file| file.read(FIXTURE_SOURCE_BYTES))
        .map_err(|failure| {
            json!({
                "file": name,
                "problem": format!("the source cannot be read from the fixture folder: {}", failure.message()),
            })
        })?;
    Ok((inputs, Some(Arc::new(bytes))))
}

/// Opens the throwaway ledger of one test of a write, and the run it holds.
async fn open_ledger(
    plan: &Plan,
    plan_hash: Digest,
    test: &TestPlan,
    tool: &a2m_core::ToolPlan,
    world: &OfflineWorld,
) -> Result<OfflineLedger, Failure> {
    let vault = world.vault.clone().ok_or_else(|| {
        codes::busy(
            codes::ENGINE_DEGRADED,
            "The vault is not open, so an offline test of a write cannot keep its own evidence.",
            "Call `doctor.run` for the fix.",
        )
    })?;
    let store = Store::open(StoreConfig::throwaway()).await?;
    store.attach(vault).await?;
    let environment = plan
        .default_environment()
        .map(|environment| environment.name.clone())
        .ok_or_else(|| {
            codes::input(
                codes::SCHEMA_INVALID,
                "The plan's default environment is not in the plan.",
                "Declare a default environment in `target.yaml`.",
            )
        })?;
    let effect = match tool.runs {
        ToolRuns::Operation(index) => plan.operation(index).map(|found| found.effect),
        ToolRuns::Pipeline(index) => plan.pipeline(index).map(|found| found.effect),
    }
    .unwrap_or(EffectClass::Irreversible);
    let new = NewRun {
        workspace: world.workspace,
        principal: world.principal,
        client_label: "offline test".to_owned(),
        tool_name: tool.name.clone(),
        tool_kind: match tool.runs {
            ToolRuns::Operation(_) => ToolKind::Operation,
            ToolRuns::Pipeline(_) => ToolKind::Pipeline,
        },
        target: plan.header.target.clone(),
        target_version: plan_hash,
        environment,
        identity: None,
        effect,
        input_hash: test.inputs.digest(),
        input: None,
        retention: RetentionClass::Short,
    };
    let fence = store
        .write(Durability::Synced, move |tx| {
            let mut ledger = tx.ledger();
            let Opened::New(opened) = ledger.open_run(new)? else {
                return Err(nothing_kept("found a run in a store it had just opened"));
            };
            ledger.new_epoch(opened.run)
        })
        .await?;
    Ok(OfflineLedger {
        store,
        fence,
        owner: OwnerRef::of(&Identity::Person(world.principal)),
    })
}

/// What the run itself shows, for `expect` to name: its end state, its
/// records verified or found unchanged, and the code it stopped with.
async fn run_facts(outcome: &JobOutcome, ledger: Option<&OfflineLedger>) -> Json {
    let state = match outcome {
        JobOutcome::Settled { .. } | JobOutcome::Left(_) | JobOutcome::LeaseEnding => None,
        other => other.run_state(),
    };
    let mut facts = json!({
        "state": state.map(|state| state.to_string()),
        "failure": outcome.failure().map(|failure| failure.code().to_string()),
    });
    if let Some(ledger) = ledger {
        let run = ledger.fence.run;
        if let Ok(tally) = ledger.store.read(move |tx| tx.ledger().tally(run)).await {
            facts["records_verified"] = json!(tally.verified + tally.unchanged);
            facts["records"] = json!({
                "verified": tally.verified,
                "unchanged": tally.unchanged,
                "acknowledged": tally.acknowledged,
                "open": tally.open,
            });
        }
    }
    facts
}

/// Runs one offline test of the draft plan through the engine, with its
/// fixture as [`read_fixture`] read it.
pub(crate) async fn run_test(
    engine: &Engine,
    plan: &Arc<Plan>,
    plan_hash: Digest,
    world: &OfflineWorld,
    test: &TestPlan,
    fixture: Option<Result<Fixture, Json>>,
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
    let fixture = match fixture {
        Some(Ok(fixture)) => fixture,
        Some(Err(wrong)) => {
            return TestOutcome::failed(
                json!({"reason": "The fixture cannot be used.", "fixture_problem": wrong}),
            );
        }
        None => {
            return TestOutcome::failed(json!({"reason": "The offline test names no fixture."}));
        }
    };
    let (input, source) = match source_of(plan, test, &fixture) {
        Ok(found) => found,
        Err(wrong) => {
            return TestOutcome::failed(
                json!({"reason": "The test's source cannot be used.", "fixture_problem": wrong}),
            );
        }
    };
    let write = writes(plan, test);
    let ledger = if write {
        match open_ledger(plan, plan_hash, test, tool, world).await {
            Ok(ledger) => Some(ledger),
            Err(failure) => {
                return TestOutcome::failed(json!({
                    "reason": "The test's own ledger could not be opened.",
                    "failure": {"class": failure.class().to_string(), "code": failure.code().to_string(), "message": failure.message()},
                }));
            }
        }
    } else {
        None
    };
    let replay = Replay::new(fixture.answers.clone());
    let (run, epoch) = match &ledger {
        Some(ledger) => (ledger.fence.run, ledger.fence.epoch),
        None => {
            // A made-up run of the test's own: it is stored nowhere.
            let mut id = [0_u8; 16];
            id.copy_from_slice(&Digest::of(test.name.as_bytes()).as_bytes()[..16]);
            (RunId::from_bytes(id), Epoch::new(1))
        }
    };
    let control = OfflineControl {
        plan: Arc::clone(plan),
        tool: tool.name.clone(),
        workspace: world.workspace,
        result: Mutex::new(None),
        ledger,
    };
    let job = JobSpec {
        run,
        workspace: world.workspace,
        attempt: 1,
        epoch,
        // A read in the sandbox's posture, where a committing step is
        // refused by the handle; a write as a run of its own ledger.
        kind: if write { JobKind::Execute } else { JobKind::Try },
        plan: plan_hash,
        tool: ToolRef::Tool(tool.name.clone()),
        environment: None,
        identity: None,
        input,
        source,
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
    let facts = run_facts(&outcome, control.ledger.as_ref()).await;
    let got = control
        .result
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
        .map(|values| crate::render::plain_values(&values))
        .unwrap_or_else(|| json!({}));
    let expected = test.expect.decode::<Json>().unwrap_or(Json::Null);
    // A test that names no state expects the run to complete.
    let expects_state = expected.get("state").is_some() && got.get("state").is_none();
    let completed = matches!(
        outcome,
        JobOutcome::Finished {
            state: RunState::Completed,
            ..
        }
    );
    let mut detail = json!({"steps": asked});
    if let Some(records) = facts.get("records") {
        detail["records"] = records.clone();
    }
    if let Some(failure) = outcome.failure() {
        detail["failure"] = json!({
            "class": failure.class().to_string(),
            "code": failure.code().to_string(),
            "message": failure.message(),
        });
    }
    if !completed && !expects_state {
        detail["reason"] = json!("The tool did not complete against its fixture.");
        if !fixture.files.is_empty() {
            detail["not_answers"] = json!(fixture.files);
        }
        return TestOutcome::failed(detail);
    }
    let differs = differences(&expected, &got, &facts);
    if write {
        let requests = request_differences(&fixture.requests, &replay.sent());
        if !requests.is_empty() {
            detail["reason"] =
                json!("The engine did not send the save the request the fixture recorded.");
            detail["request_differences"] = json!(requests);
            if !differs.is_empty() {
                detail["differences"] = json!(differs);
            }
            return TestOutcome::failed(detail);
        }
    }
    if differs.is_empty() {
        TestOutcome {
            state: "passed",
            detail,
        }
    } else {
        detail["reason"] = json!("The result is not the one the test states.");
        detail["differences"] = json!(differs);
        TestOutcome::failed(detail)
    }
}

/// The publish gates the documents name (spec section 403), each with
/// whether this build runs it. Only the schema and lint gate and the
/// offline replay are built; a publish is refused unless both pass, and
/// the evidence and the person's page say of every other gate that it did
/// not run (decision 0180).
pub(crate) const GATES: [(&str, bool); 12] = [
    ("schema_and_lint", true),
    ("offline_replay", true),
    ("contract", false),
    ("determinism", false),
    ("binding_equivalence", false),
    ("write_round_trip", false),
    ("idempotency", false),
    ("resume", false),
    ("fault_injection", false),
    ("performance", false),
    ("agent_usability", false),
    ("human_review", false),
];

/// The gates of [`GATES`] this build does not run.
pub(crate) fn gates_not_built() -> Vec<&'static str> {
    GATES
        .iter()
        .filter(|(_, built)| !built)
        .map(|(name, _)| *name)
        .collect()
}

/// What running the publish gates on a target's drafts found.
pub(crate) struct GateRun {
    /// Each gate that ran, with what it found, in the form `test.run`
    /// returns.
    pub(crate) gates: Json,
    /// Whether every gate that ran passed, and every offline test ran and
    /// passed.
    pub(crate) passed: bool,
    /// The offline tests that did not pass, each with its state and what
    /// to do about it.
    pub(crate) missing: Vec<Json>,
}

impl GateRun {
    /// The evidence a published version keeps: each gate that ran with its
    /// result, and each the documents name that did not run.
    pub(crate) fn evidence(&self) -> Json {
        json!({
            "gates": self.gates,
            "not_run": gates_not_built(),
            "passed": self.passed,
        })
    }

    /// One line per gate, for the page a person decides a publish on.
    pub(crate) fn lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for gate in self.gates.as_array().into_iter().flatten() {
            let name = gate["gate"].as_str().unwrap_or_default();
            let state = gate["state"].as_str().unwrap_or_default();
            let line = match gate["tests"].as_array() {
                Some(tests) => {
                    let passed = tests.iter().filter(|test| test["state"] == "passed").count();
                    format!(
                        "Gate {name}: {state}, {passed} of {} offline tests passed.",
                        tests.len()
                    )
                }
                None => format!("Gate {name}: {state}."),
            };
            lines.push(line);
        }
        lines.push(format!(
            "Not built, so these gates did not run: {}.",
            gates_not_built().join(", ")
        ));
        lines
    }

    /// The refusal of a publish whose gates did not pass, naming each test
    /// that is missing or did not pass.
    pub(crate) fn refusal(&self, target: &a2m_core::TargetName) -> Failure {
        let names: Vec<String> = self
            .missing
            .iter()
            .map(|test| {
                format!(
                    "{} ({})",
                    test["test"].as_str().unwrap_or_default(),
                    test["state"].as_str().unwrap_or_default()
                )
            })
            .collect();
        codes::input(
            codes::PUBLISH_GATES_NOT_PASSED,
            format!(
                "The target `{target}` is not published: a version is published only when its offline tests have run and passed, and these did not: {}.",
                names.join(", ")
            ),
            format!(
                "Call `test.run` with the target `{target}` to see what each test found, correct the test, its fixture, or the binding, and publish again."
            ),
        )
    }
}

/// Runs the publish gates this build has on a compiled target: the schema
/// and lint gate, which compiling for publishing passed, and every offline
/// test of the plan against its fixture.
///
/// # Errors
///
/// The store's and the engine's failures; `busy`/`engine_degraded` when
/// the tests were interrupted.
pub(crate) async fn run_gates(
    kernel: &crate::kernel::Kernel,
    project_folder: std::path::PathBuf,
    target: &a2m_core::TargetName,
    compiled: &a2m_files::Compiled,
    world: OfflineWorld,
) -> Result<GateRun, Failure> {
    use crate::handlers::build::{off_thread, problems_json, target_folder};
    let plan = Arc::new(compiled.plan.clone());
    let (named, tests) = (target.clone(), plan.tests.clone());
    // Every fixture is read once, off the engine's threads. The compiler
    // names a fixture by its folder inside the target's, `fixtures/<name>`.
    let fixtures = off_thread(move || {
        let folder = target_folder(&project_folder, &named)?;
        Ok(tests
            .iter()
            .map(|test| {
                test.fixture.as_deref().map(|fixture| {
                    folder
                        .join(fixture)
                        .map_err(|failure| json!({"folder": fixture, "problem": failure.message()}))
                        .and_then(read_fixture)
                })
            })
            .collect::<Vec<_>>())
    })
    .await?;
    let engine = kernel.engine().await?;
    // The engine's future is not `Send`: a driver's prepared state is held
    // across its waits. So the tests run on a thread of their own, driven
    // by this same runtime, as the runner loop runs a job.
    let (runs_plan, hash) = (Arc::clone(&plan), compiled.encoded.hash);
    let runtime = tokio::runtime::Handle::current();
    let outcomes = tokio::task::spawn_blocking(move || {
        runtime.block_on(async move {
            let mut outcomes = Vec::new();
            for (test, fixture) in runs_plan.tests.iter().zip(fixtures) {
                outcomes.push(run_test(&engine, &runs_plan, hash, &world, test, fixture).await);
            }
            outcomes
        })
    })
    .await
    .map_err(|_| {
        codes::busy(
            codes::ENGINE_DEGRADED,
            "The offline tests were interrupted.",
            "Call `test.run` again.",
        )
    })?;
    let mut results = Vec::new();
    let mut missing = Vec::new();
    for (test, outcome) in plan.tests.iter().zip(outcomes) {
        let tool = plan
            .tools
            .get(test.tool.get())
            .map(|tool| tool.name.to_string())
            .unwrap_or_default();
        let mut shown = json!({
            "test": test.name,
            "tool": tool,
            "kind": test.kind.to_string(),
            "fixture": test.fixture,
            "state": outcome.state,
        });
        if let (Json::Object(shown), Json::Object(detail)) = (&mut shown, outcome.detail) {
            shown.extend(detail);
        }
        // A live test belongs to the write round trip, which is not built:
        // the gate excuses it. Every offline test must have run and
        // passed.
        if test.kind == TestKind::Offline && shown["state"] != "passed" {
            missing.push(json!({
                "test": test.name,
                "tool": tool,
                "state": shown["state"],
                "next": {"action": "test.run", "input": {"target": target.as_str()}},
            }));
        }
        results.push(shown);
    }
    let offline: Vec<&Json> = results
        .iter()
        .filter(|test| test["kind"] == "offline")
        .collect();
    let replay_state = if offline.iter().any(|test| test["state"] == "failed") {
        "failed"
    } else if offline.iter().all(|test| test["state"] == "passed") {
        "passed"
    } else {
        "incomplete"
    };
    Ok(GateRun {
        gates: json!([
            {"gate": "schema_and_lint", "state": "passed", "problems": problems_json(&compiled.warnings)},
            {"gate": "offline_replay", "state": replay_state, "tests": results},
        ]),
        passed: missing.is_empty(),
        missing,
    })
}
