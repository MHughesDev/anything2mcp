//! Carrying out a tool: a read through the loop once, a write through its
//! preview, the approval check, reconcile, and the write path, and a
//! pipeline as its steps in order.

use std::sync::Arc;
use std::time::Duration;

use a2m_core::{
    BindingPlan, Digest, EffectClass, ExprIndex, Failure, FailureClass, IdentityStrategy,
    InputPlan, OperationIndex, OperationName, OperationPlan, REDACTED, RecordKey, RecordState,
    RunState, StepArg, Timestamp, ToolRuns, Value, Values, WriteEvidence,
};
use a2m_expr::names;
use a2m_ledger::{
    Acted, ActedToken, BlobKind, BlobRef, PlannedRecord, ReadBack, Reconciled, RecordIntent,
    RecordRef, RetentionClass, Settled,
};
use a2m_provider::{Observed, Posture, Sent, StepFailure, StepOutcome};
use a2m_sources::{LoadedSource, OPERATION_READER, Sources};
use a2m_telemetry::Phase;

use crate::codes;
use crate::compare::{self, Field};
use crate::control::{Clearance, EngineEvent, Evidence, GuardQuestion, Preview};
use crate::diff::{Diff, DiffAction, DiffBinding, DiffField, DiffRecord};
use crate::job::{JobKind, RecordSnapshot};
use crate::loaded::{Loaded, Scope};
use crate::run::{Candidate, RecordCx, Run, StepCx, Stop, masked, merge, operation};
use crate::values::encode_values;

/// How the work on a tool ended, when nothing stopped it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Done {
    /// Every step ran; every record is verified or unchanged.
    Completed,
    /// The diff is stored and waits for a person.
    NeedsApproval,
    /// Every record in doubt was read and settled; nothing was written.
    Settled {
        /// How many records are still to be written.
        remaining: u32,
    },
}

/// One step of a tool: a pipeline's step, or the one operation of a tool
/// that runs an operation.
struct StepShape<'p> {
    seq: u32,
    id: &'p str,
    operation: OperationIndex,
    /// The operation's inputs, each from an expression; `None` when the
    /// tool's own inputs are the operation's.
    with: Option<&'p [StepArg]>,
    for_each: Option<ExprIndex>,
    when: Option<ExprIndex>,
    expect: Option<ExprIndex>,
    writes: bool,
}

/// A record a write step would touch, as the preview or the ledger has it.
struct Planned {
    record: RecordRef,
    operation: OperationIndex,
    inputs: Values,
    key: RecordKey,
    fields: Vec<Field>,
    /// The normalized intended values, as they are compared.
    intended: Values,
    digest: Digest,
    /// The values seen before the write, as they are compared; `None` when
    /// the record did not exist.
    before: Option<Values>,
    /// What the system gave when this job last read the record.
    current: Option<Values>,
    /// When this job last read the record, on the engine's clock.
    seen_at: Option<Duration>,
    /// The driver context this job last read the record in; `None` when
    /// the record was not read, because nothing could be looked up.
    seen_in: Option<u64>,
    state: RecordState,
    stamp: Option<String>,
    /// The position of the binding that writes it.
    binding: usize,
    outputs: Values,
    /// What is known of the record beyond its inputs: the values recorded
    /// before its commit point, and those its write's answer carried. The
    /// steps that read the record find them under `result`.
    known: Values,
    /// The key the system assigned, once known. From then on the record is
    /// read by it.
    assigned: Option<RecordKey>,
    /// Whether values were recorded before the record's commit point, by
    /// which it is read instead of being looked up.
    recorded: bool,
    /// True while an input that is part of the record's key waits for a
    /// record written earlier in the run.
    pending: bool,
    /// True when a value the ledger holds of the record came back masked
    /// by the redactor. Such a value names nothing in the system: it is
    /// never used as an argument, a key, or an output.
    masked: bool,
    /// True when the record's key is its place in the run, the engine's own
    /// name for it, which is never handed to a step as a key.
    place_keyed: bool,
    /// What the record's earlier steps returned for reconcile, within this
    /// job or, on resume, as the ledger kept it before the commit point.
    recorded_values: Values,
    /// True when the record's write was accepted by the system and cannot
    /// be read back: it ends `acted`, never `verified`.
    acknowledged: bool,
    /// The positions of the bindings the approval lets write the record:
    /// the one its diff record names, or every one it could take when its
    /// binding is chosen only once its key is known. Empty until a diff is
    /// known, and a record with none is never written.
    approved: Vec<usize>,
}

/// The key a step that does the work of a record is given: the record's
/// key from its key inputs; never its place in the run.
fn act_key(record: &Planned) -> Option<&RecordKey> {
    (!record.place_keyed).then_some(&record.key)
}

/// The key a step that reads a record is given (decision 0161): the key
/// the system assigned, or the key of the record's key inputs. `None` when
/// the record has neither; never its place in the run, which names nothing
/// in the system.
fn read_key(found: &OperationPlan, record: &Planned) -> Option<RecordKey> {
    if let Some(assigned) = &record.assigned {
        return Some(assigned.clone());
    }
    if !record.place_keyed {
        return Some(record.key.clone());
    }
    if record.pending || found.identity.key.is_empty() {
        return None;
    }
    // A record planned while its key waited for an earlier record: its
    // inputs are whole now, and so is its key.
    key_of(found, &record.inputs, record.record).ok()
}

/// What a step of the record is handed as recorded: what the ledger kept,
/// and what the steps of this pass have returned so far.
fn recorded_with(record: &Planned, more: &Values) -> Values {
    let mut recorded = record.recorded_values.clone();
    for (name, value) in more.iter() {
        recorded.insert(name.to_owned(), value.clone());
    }
    recorded
}

/// The inputs of one call of a step.
struct Call {
    inputs: Values,
    /// True when an input that is part of the key could not be worked out
    /// yet, because it comes from a record that is still to be written.
    pending: bool,
}

/// Whether a binding has nothing to read its record with: its only
/// evidence is the system's acknowledgement, and it has no read-back step
/// (decision 0196). Such a record is never read before it is written,
/// nothing is compared after, and a lookup is asked of it only in
/// reconcile, for whether it landed, never for its values.
fn cannot_read(binding: &BindingPlan) -> bool {
    binding.evidence == WriteEvidence::Acknowledged && binding.read_back.is_empty()
}

fn writes(operation: &OperationPlan) -> bool {
    operation
        .bindings
        .iter()
        .any(|binding| binding.act.iter().any(|step| step.commits))
}

fn stale(what: &str) -> Failure {
    codes::fail(
        FailureClass::Stale,
        codes::CHANGED_SINCE_PREVIEW,
        what,
        "Preview the run again and approve the new diff.",
    )
}

/// The classes that settle a record as not written: the ledger releases
/// its lock, the run ends `failed`, and nothing reads the record again.
/// They are right only while nothing was sent, or when the system itself
/// refused the write.
const SETTLES: [FailureClass; 3] = [
    FailureClass::Policy,
    FailureClass::Stale,
    FailureClass::Input,
];

/// The approval was given for something other than what would now be
/// written: nothing is sent, and a new preview and approval are needed.
fn stale_approval(what: &str) -> Failure {
    codes::fail(
        FailureClass::Policy,
        codes::APPROVAL_STALE,
        what,
        "Preview the run again and approve the new diff.",
    )
}

fn settles(class: FailureClass) -> bool {
    SETTLES.contains(&class)
}

/// The failure of a record whose write was sent and which could not be
/// verified: the write is in doubt, whatever failed afterwards. The class
/// and code it stands in for are named, and stay in the step trace.
fn in_doubt(what: &str, replaced: &Failure) -> Failure {
    codes::fail(
        FailureClass::PossibleCommit,
        codes::NO_CONFIRMATION,
        format!(
            "A write was sent, and {what} failed before the record was verified (`{}/{}`).",
            replaced.class(),
            replaced.code()
        ),
        "Reconcile the run: read the record back before anything is written again.",
    )
    .carrying_said(replaced)
}

/// What the failure of the committing step itself becomes, and whether the
/// record is in doubt after it.
///
/// Nothing sent: the failure stands. Fate unknown: in doubt. Answered: the
/// system's own refusal of the write stands, and it is told by its class,
/// `rejected` for the input and `stale` for the version sent with the
/// write. An answered failure of a class that would settle the record and
/// is not such a refusal (`policy`, `input`) says only that the answer was
/// not accepted, so the write is in doubt. Every other class leaves the
/// record in doubt at the ledger as it is.
fn commit_failure(failure: StepFailure) -> Failure {
    let StepFailure { failure, sent } = failure;
    let class = failure.class();
    match sent {
        Sent::NotSent => failure,
        Sent::Unknown if class == FailureClass::PossibleCommit => failure,
        Sent::Unknown => in_doubt("the step that saves the record", &failure),
        Sent::Answered if matches!(class, FailureClass::Policy | FailureClass::Input) => in_doubt(
            "reading the answer to the step that saves the record",
            &failure,
        ),
        Sent::Answered => failure,
    }
}

/// What the failure of a provider's step becomes once the record's write
/// was sent: the write is in doubt. Two classes stand, because each keeps
/// the record in doubt and says more: `possible_commit` itself, and
/// `session_expired`, which rests the run until a person signs in.
fn after_send(failure: Failure, what: &str) -> Failure {
    match failure.class() {
        FailureClass::PossibleCommit | FailureClass::SessionExpired => failure,
        _ => in_doubt(what, &failure),
    }
}

/// What the failure of a confirm step becomes. The system's own answer
/// that it refused the write stands: a failure the system answered with,
/// of the class `rejected`. It does not settle the record: the lock is
/// kept, and a resume reads the record before anything is written.
fn confirm_failure(failure: StepFailure) -> Failure {
    if failure.sent == Sent::Answered && failure.failure.class() == FailureClass::Rejected {
        failure.failure
    } else {
        after_send(failure.failure, "the confirmation")
    }
}

/// A value as it is stored or shown: with every registered secret in it
/// replaced by the redaction mark.
fn kept_value(run: &Run<'_>, value: &Value) -> Value {
    run.engine.deps.redact.redact_value(value)
}

/// Values as they are stored or shown. Every value the engine hands to
/// `Control`, as evidence, in the diff, or as a result, passes through
/// here, whatever passed the redactor before it; a name is redacted too,
/// because a system chooses the names it returns.
fn kept(run: &Run<'_>, values: &Values) -> Values {
    run.engine.deps.redact.redact_values(values)
}

/// The steps of a binding in the order a write runs them.
fn steps_of(binding: &BindingPlan) -> impl Iterator<Item = &a2m_core::Step> {
    binding
        .act
        .iter()
        .chain(&binding.confirm)
        .chain(&binding.read_back)
}

/// A binding saves once in one call: this engine carries out one commit
/// point per call. Checked for the binding that is about to write, before
/// anything is sent.
fn one_save(operation: &OperationPlan, binding: Option<&BindingPlan>) -> Result<(), Failure> {
    let commits = binding.map_or(0, |binding| {
        binding.act.iter().filter(|step| step.commits).count()
    });
    if commits == 1 {
        return Ok(());
    }
    Err(codes::fail(
        FailureClass::Busy,
        codes::JOB_KIND_NOT_BUILT,
        format!(
            "The binding of `{}` saves {commits} times in one call, and this engine carries out one save per call.",
            operation.name
        ),
        "Split the operation into one operation for each save.",
    ))
}

/// The refusal of a write in the sandbox to a record that is not a
/// declared test record.
fn not_a_test_record() -> Failure {
    codes::fail(
        FailureClass::Policy,
        codes::SANDBOX_READ_ONLY,
        "The sandbox writes only to the test records the environment declares, and this record is not one.",
        "Declare the record under the environment's test records, or try a read.",
    )
}

/// The record's key: its identity inputs, or, for an operation that names
/// none, its place in the run.
fn key_of(
    operation: &OperationPlan,
    inputs: &Values,
    record: RecordRef,
) -> Result<RecordKey, Failure> {
    let mut parts = Vec::new();
    for index in &operation.identity.key {
        let input = operation.inputs.get(index.get()).ok_or_else(|| {
            codes::job_invalid("An operation's key names an input the operation does not have.")
        })?;
        let value = inputs.get(&input.name).cloned().ok_or_else(|| {
            codes::fail(
                FailureClass::Input,
                codes::INPUT_INVALID,
                format!(
                    "`{}` is part of the record's key and has no value.",
                    input.name
                ),
                "Give every input that is part of the key.",
            )
        })?;
        parts.push(value);
    }
    if parts.is_empty() {
        return place_key(operation, record);
    }
    RecordKey::new(parts).map_err(Failure::from)
}

/// The key of a record that has none of its own yet: its place in the run.
/// It is the key of a record whose operation names no key input, and of a
/// record whose key waits for a record written earlier in the run.
fn place_key(operation: &OperationPlan, record: RecordRef) -> Result<RecordKey, Failure> {
    RecordKey::new(vec![
        Value::Text(operation.name.to_string()),
        Value::Int(i64::from(record.step_seq)),
        Value::Int(i64::from(record.record_seq)),
    ])
    .map_err(Failure::from)
}

/// A key the system assigned, read back from the text the ledger keeps.
fn parse_key(text: &str) -> Option<RecordKey> {
    let parts: Vec<Value> = serde_json::from_str(text).ok()?;
    RecordKey::new(parts).ok()
}

/// The outputs an operation declares, from what its steps observed, each
/// normalized by its field type.
fn outputs_of(
    loaded: &Loaded,
    operation: &OperationPlan,
    observed: &Values,
    all_required: bool,
) -> Result<Values, Failure> {
    let unexpected = |what: String| {
        codes::fail(
            FailureClass::UnknownVariant,
            codes::OUTPUT_UNEXPECTED,
            what,
            "Compare the operation's outputs in the target files with what the system returns.",
        )
    };
    let mut out = Values::new();
    for output in &operation.outputs {
        let Some(raw) = observed.get(&output.name) else {
            if all_required {
                return Err(unexpected(format!(
                    "The operation `{}` returned no value for its output `{}`.",
                    operation.name, output.name
                )));
            }
            continue;
        };
        if let Value::Text(text) = raw
            && text.contains(REDACTED)
            && !matches!(
                output.ty,
                a2m_core::ValueType::Text | a2m_core::ValueType::Choice
            )
        {
            // A number, a date, a moment, or a length of time that held a
            // registered secret reaches the result as the mask's text. It
            // is the system's value, masked, and no failure of the run.
            out.insert(output.name.clone(), Value::Text(REDACTED.to_owned()));
            continue;
        }
        let value = match (raw, &output.ty) {
            // A list output is normalized one item at a time, each by the
            // output's field type; a `null` item, a row with no value,
            // stays `null`. An item that cannot be read fails the read by
            // its position, never by its value (decision 0180).
            (Value::List(items), a2m_core::ValueType::List(_)) => {
                let mut normalized = Vec::with_capacity(items.len());
                for (at, item) in items.iter().enumerate() {
                    if item.is_null() {
                        normalized.push(Value::Null);
                        continue;
                    }
                    normalized.push(loaded.normalize(output.field_type, item).map_err(
                        |failure| {
                            unexpected(format!(
                                "The item {} of the output `{}` of `{}` cannot be read as its type ({}).",
                                at + 1,
                                output.name,
                                operation.name,
                                failure.code()
                            ))
                        },
                    )?);
                }
                Value::List(normalized)
            }
            _ => loaded
                .normalize(output.field_type, raw)
                .map_err(|failure| {
                    unexpected(format!(
                        "The output `{}` of `{}` cannot be read as its type ({}).",
                        output.name,
                        operation.name,
                        failure.code()
                    ))
                })?,
        };
        if !value.is_null() && !value.has_type(&output.ty) {
            return Err(unexpected(format!(
                "The output `{}` of `{}` is not a {}.",
                output.name,
                operation.name,
                output.ty.name()
            )));
        }
        out.insert(output.name.clone(), value);
    }
    Ok(out)
}

/// The fields of what a read returns, for the trace: the outputs that have
/// a field type.
fn output_fields(loaded: &Loaded, operation: &OperationPlan) -> Vec<Field> {
    operation
        .outputs
        .iter()
        .filter_map(|output| {
            output.field_type.map(|field_type| Field {
                name: output.name.clone(),
                field_type,
                sensitivity: loaded.sensitivity(Some(field_type)),
            })
        })
        .collect()
}

fn shape<'p>(
    loaded: &'p Loaded,
    runs: ToolRuns,
) -> Result<(Vec<StepShape<'p>>, &'p [InputPlan]), Failure> {
    match runs {
        ToolRuns::Operation(index) => {
            let found = operation(loaded, index)?;
            Ok((
                vec![StepShape {
                    seq: 0,
                    id: "call",
                    operation: index,
                    with: None,
                    for_each: None,
                    when: None,
                    expect: None,
                    writes: writes(found),
                }],
                &found.inputs,
            ))
        }
        ToolRuns::Pipeline(index) => {
            let pipeline = loaded.plan.pipeline(index).ok_or_else(|| {
                codes::job_invalid("The tool names a pipeline the plan does not hold.")
            })?;
            let mut steps = Vec::with_capacity(pipeline.steps.len());
            for (position, step) in pipeline.steps.iter().enumerate() {
                steps.push(StepShape {
                    seq: u32::try_from(position).unwrap_or(u32::MAX),
                    id: &step.id,
                    operation: step.operation,
                    with: Some(&step.with),
                    for_each: step.for_each,
                    when: step.when,
                    expect: step.expect,
                    writes: writes(operation(loaded, step.operation)?),
                });
            }
            Ok((steps, &pipeline.inputs))
        }
    }
}

/// Everything a tool's steps share while they run.
struct Tool<'p> {
    name: OperationName,
    steps: Vec<StepShape<'p>>,
    /// The scope of the tool's own expressions: its inputs, its source, and
    /// the results of the steps that have run.
    scope: Scope,
    /// What the tool returns: the result of the last step that ran.
    last: Values,
}

impl Tool<'_> {
    /// The inputs of each call of a step: one, or one for each row.
    ///
    /// With `may_wait`, an input whose expression reads `steps` and cannot
    /// be worked out is left out, and its call is marked as pending: it
    /// comes from a record that is still to be written. Such an input must
    /// be part of the record's key, because a value that is written has to
    /// be known when its diff is approved.
    fn calls(
        &self,
        loaded: &Loaded,
        step: &StepShape<'_>,
        may_wait: bool,
    ) -> Result<Vec<Call>, Failure> {
        if let Some(when) = step.when
            && !loaded.holds(when, &self.scope)?
        {
            return Ok(Vec::new());
        }
        let found = operation(loaded, step.operation)?;
        let Some(with) = step.with else {
            // The tool's inputs were validated as this operation's inputs.
            return Ok(vec![Call {
                inputs: match &self.scope.input {
                    Value::Record(values) => values.clone(),
                    _ => Values::new(),
                },
                pending: false,
            }]);
        };
        let rows = match step.for_each {
            Some(rows) => match loaded.eval(rows, &self.scope)? {
                Value::List(rows) => rows,
                _ => {
                    return Err(codes::job_invalid(format!(
                        "The step `{}` repeats over something that is not a list of rows.",
                        step.id
                    )));
                }
            },
            None => vec![Value::Null],
        };
        let mut calls = Vec::with_capacity(rows.len());
        for row in rows {
            let mut scope = self.scope.clone();
            scope.row = row;
            let mut given = Values::new();
            let mut waiting: Vec<String> = Vec::new();
            for arg in with {
                let waits = may_wait && loaded.reads(arg.value, names::STEPS);
                match loaded.eval(arg.value, &scope) {
                    Ok(value) if waits && value.is_null() => waiting.push(arg.name.clone()),
                    Ok(value) => {
                        given.insert(arg.name.clone(), value);
                    }
                    Err(_) if waits => waiting.push(arg.name.clone()),
                    Err(failure) => return Err(failure),
                }
            }
            for name in &waiting {
                let in_key = found.identity.key.iter().any(|index| {
                    found
                        .inputs
                        .get(index.get())
                        .is_some_and(|input| &input.name == name)
                });
                if !in_key {
                    return Err(codes::fail(
                        FailureClass::Input,
                        codes::INPUT_INVALID,
                        format!(
                            "The input `{name}` of the step `{}` comes from a record that is still to be written, and it is not part of the record's key, so its value cannot be shown for approval.",
                            step.id
                        ),
                        "Pass a later step only the key of a record an earlier step writes.",
                    ));
                }
            }
            calls.push(Call {
                inputs: loaded.inputs_waiting(&found.inputs, &given, &waiting)?,
                pending: !waiting.is_empty(),
            });
        }
        Ok(calls)
    }

    /// Keeps a step's result for the steps after it and for the tool.
    fn keep(&mut self, step: &StepShape<'_>, mut results: Vec<Values>) {
        let result = if step.for_each.is_some() {
            Value::List(results.iter().cloned().map(Value::Record).collect())
        } else {
            Value::Record(results.first().cloned().unwrap_or_default())
        };
        if let Value::Record(steps) = &mut self.scope.steps {
            steps.insert(step.id.to_owned(), Values::new().with("result", result));
        }
        if let Some(last) = results.pop() {
            self.last = last;
        }
    }
}

/// Reads the source of a pipeline through `a2m-sources`, and gives its rows
/// as the value expressions read under `source`.
///
/// Parsing a source of the size the limits allow takes long enough to hold
/// up everything else on its thread, so it runs on a thread of its own. A
/// reader is a provider's code: a panic in it ends the read and not the
/// engine.
async fn read_source(
    run: &Run<'_>,
    loaded: &Loaded,
    runs: ToolRuns,
) -> Result<(Value, u32), Failure> {
    let ToolRuns::Pipeline(index) = runs else {
        return Ok((Value::Null, 0));
    };
    let Some(source) = loaded
        .plan
        .pipeline(index)
        .and_then(|pipeline| pipeline.source.as_ref())
    else {
        return Ok((Value::Null, 0));
    };
    if source.reader == OPERATION_READER {
        return Err(codes::fail(
            FailureClass::Busy,
            codes::JOB_KIND_NOT_BUILT,
            "The tool reads its rows from another operation's result, and this engine does not carry that out yet.",
            "Give the rows as a file, or call the operations one after the other.",
        ));
    }
    let Some(bytes) = run.job.source.as_ref() else {
        return Err(codes::fail(
            FailureClass::Input,
            codes::INPUT_INVALID,
            "The tool reads a source and the job carries none.",
            "Call the tool with its source input.",
        ));
    };
    let definition = LoadedSource::load(&loaded.plan, source)?;
    let builtin = Sources::builtin();
    let sources = if builtin
        .readers()
        .any(|reader| reader.as_str() == definition.reader())
    {
        builtin
    } else {
        Sources::new(run.engine.deps.registry.source_readers())?
    };
    let bytes = Arc::clone(bytes);
    let limits = run.engine.deps.settings.source_limits;
    let reading =
        tokio::task::spawn_blocking(move || sources.read(&definition, &bytes, &limits)).await;
    let records = match reading {
        Ok(read) => read.map_err(Failure::from)?,
        Err(_) => {
            return Err(codes::fail(
                FailureClass::UnknownVariant,
                codes::DRIVER_PANICKED,
                "The reader of the tool's source panicked.",
                "Report this to the reader's author; nothing was sent to the system.",
            ));
        }
    };
    let count = u32::try_from(records.rows.len()).unwrap_or(u32::MAX);
    let rows = records
        .rows
        .into_iter()
        .map(|row| Value::Record(row.fields))
        .collect();
    Ok((
        Value::Record(
            Values::new()
                .with("rows", Value::List(rows))
                .with("template", records.template),
        ),
        count,
    ))
}

/// Carries out the tool the job names.
pub(crate) async fn run_tool(run: &mut Run<'_>, name: &OperationName) -> Result<Done, Stop> {
    let loaded = Arc::clone(&run.loaded);
    let tool = loaded
        .plan
        .tool(name)
        .ok_or_else(|| codes::job_invalid(format!("The plan holds no tool named `{name}`.")))?;
    let (steps, declared) = shape(&loaded, tool.runs)?;
    let writing = steps.iter().any(|step| step.writes);
    let (effect, hazard, limits) = match tool.runs {
        ToolRuns::Operation(index) => {
            let found = operation(&loaded, index)?;
            (
                found.effect,
                found.hazard,
                a2m_core::PipelineLimits::default(),
            )
        }
        ToolRuns::Pipeline(index) => {
            let pipeline = loaded.plan.pipeline(index).ok_or_else(|| {
                codes::job_invalid("The tool names a pipeline the plan does not hold.")
            })?;
            (pipeline.effect, pipeline.hazard, pipeline.limits)
        }
    };
    if run.job.kind == JobKind::Canary && writing {
        return Err(Stop::Fail(codes::job_invalid(
            "A canary runs a read, and this tool writes.",
        )));
    }
    if writing && run.job.kind != JobKind::Reconcile {
        // Guard: a write needs a kernel that is `ok`.
        run.may_write()?;
    }
    if run.job.snapshot.is_none() {
        run.moved(RunState::Validating).await?;
    }
    let inputs = loaded.inputs(declared, &run.job.input)?;
    let (source, rows) = read_source(run, &loaded, tool.runs).await?;
    if let Some(most) = limits.max_records
        && rows > most
    {
        return Err(Stop::Fail(codes::fail(
            FailureClass::Policy,
            codes::RECORD_LIMIT,
            format!("The source holds {rows} records and the tool allows {most} in one run."),
            "Split the source, or raise the pipeline's limit and publish again.",
        )));
    }
    run.control
        .decide(&GuardQuestion {
            tool: name.clone(),
            environment: run.environment.clone(),
            effect,
            hazard,
            writes: writing && matches!(run.job.kind, JobKind::Execute | JobKind::Try),
            records: writing.then_some(rows.max(1)),
        })
        .await?;
    if let Some(longest) = limits.max_duration_ms {
        run.time_limit = Some(run.engine.deps.clock.elapsed() + Duration::from_millis(longest));
    }
    let mut scope = run.base.with_input(&inputs);
    scope.source = source;
    let mut tool = Tool {
        name: name.clone(),
        steps,
        scope,
        last: Values::new(),
    };
    if writing {
        write_tool(run, &loaded, &mut tool).await
    } else {
        if run.job.kind == JobKind::Reconcile {
            return Ok(Done::Settled { remaining: 0 });
        }
        run.moved(RunState::Running).await?;
        let all = tool.steps.len();
        read_steps_of(run, &loaded, &mut tool, 0, all).await?;
        run.control
            .send(EngineEvent::Result(kept(run, &tool.last)))
            .await?;
        Ok(Done::Completed)
    }
}

/// Runs the read steps of a tool from `from` up to `to`, checking each
/// `expect`.
async fn read_steps_of(
    run: &mut Run<'_>,
    loaded: &Loaded,
    tool: &mut Tool<'_>,
    from: usize,
    to: usize,
) -> Result<(), Stop> {
    for position in from..to {
        let step = &tool.steps[position];
        if step.writes {
            continue;
        }
        let found = operation(loaded, step.operation)?;
        run.tell(EngineEvent::StepStarted {
            step_seq: step.seq,
            operation: found.name.clone(),
        })
        .await;
        let calls = tool.calls(loaded, step, false)?;
        let mut results = Vec::with_capacity(calls.len());
        for call in &calls {
            let outputs = read_operation(run, loaded, found, &call.inputs).await?;
            if let Some(expect) = step.expect {
                let mut scope = tool.scope.clone();
                scope.result = Value::Record(outputs.clone());
                if !loaded.holds(expect, &scope)? {
                    return Err(Stop::Fail(codes::fail(
                        FailureClass::UnknownVariant,
                        codes::EXPECT_FAILED,
                        format!(
                            "What the step `{}` expects of its result does not hold.",
                            step.id
                        ),
                        "Look at the record in the system; the tool runs only when its expectation holds.",
                    )));
                }
            }
            results.push(outputs);
        }
        let step = &tool.steps[position];
        let kept = StepShape { ..*step };
        tool.keep(&kept, results);
    }
    Ok(())
}

/// One read through the loop: choose, prepare, act, confirm, and record the
/// result. Falls to the next qualifying binding after `drift` or a
/// `transient` failure that outlasted its retries.
async fn read_operation(
    run: &mut Run<'_>,
    loaded: &Loaded,
    found: &OperationPlan,
    inputs: &Values,
) -> Result<Values, Stop> {
    run.checkpoint(Duration::ZERO)?;
    let scope = run.base.with_input(inputs);
    let (candidates, passed_over) = run.candidates(found, &scope)?;
    let fields = output_fields(loaded, found);
    let count = candidates.len();
    for (position, candidate) in candidates.iter().enumerate() {
        run.chosen(&found.name, candidate, &passed_over).await;
        let binding = &found.bindings[candidate.binding];
        let mut asked_again = false;
        let failure = loop {
            match read_pass(
                run,
                loaded,
                found,
                binding,
                candidate,
                &scope,
                &fields,
                asked_again,
            )
            .await
            {
                Ok(values) => return Ok(values),
                Err((failure, held)) => {
                    // The session ended under the pass: tell the keeper,
                    // and ask for a lease once more.
                    if failure.class() == FailureClass::SessionExpired
                        && !asked_again
                        && let Some(held) = held
                    {
                        run.report_expiry(held).await;
                        asked_again = true;
                        continue;
                    }
                    if let Some(held) = held {
                        run.poison(held);
                    }
                    break failure;
                }
            }
        };
        let falls_back = matches!(
            failure.class(),
            FailureClass::Drift | FailureClass::Transient
        );
        if failure.class() == FailureClass::Drift {
            run.tell(EngineEvent::BindingHealth {
                operation: found.name.clone(),
                mode: binding.mode.clone(),
                surface: loaded
                    .plan
                    .surface(binding.surface)
                    .map_or_else(|| run.surface_of(0), |surface| surface.name.clone()),
                failure: Some((failure.class(), failure.code().clone())),
            })
            .await;
        }
        if !falls_back || position + 1 == count {
            return Err(Stop::Fail(failure));
        }
    }
    Err(Stop::Fail(codes::fail(
        FailureClass::Busy,
        codes::NO_BINDING,
        format!(
            "The operation `{}` has no binding that may act.",
            found.name
        ),
        "Verify a binding of the operation and publish the target again.",
    )))
}

/// One pass of a read. The failure comes with the held context it happened
/// in, when one was prepared.
#[allow(
    clippy::too_many_arguments,
    reason = "one pass reads everything the choice made; a struct would only rename the list"
)]
async fn read_pass(
    run: &mut Run<'_>,
    loaded: &Loaded,
    found: &OperationPlan,
    binding: &BindingPlan,
    candidate: &Candidate,
    scope: &Scope,
    fields: &[Field],
    again: bool,
) -> Result<Values, (Failure, Option<usize>)> {
    let held = run
        .hold(&found.name, binding, candidate, again, false)
        .await
        .map_err(|failure| (failure, None))?;
    let failed = |failure: Failure| (failure, Some(held));
    let steps = binding.act.len() + binding.confirm.len() + binding.read_back.len();
    run.budget(held, steps).await.map_err(failed)?;
    let mut scope = scope.clone();
    let mut seen = Observed::default();
    for (phase, steps) in [
        (Phase::Act, &binding.act),
        (Phase::Confirm, &binding.confirm),
        (Phase::ReadBack, &binding.read_back),
    ] {
        let observed = run
            .read_steps(held, phase, steps, &mut scope, None, fields)
            .await
            .map_err(|failure| failed(failure.failure))?;
        match observed {
            Some(observed) => {
                let mut into = Scope::new(Value::Null);
                merge(&mut into, &mut seen, observed);
            }
            None => {
                return Err(failed(codes::fail(
                    FailureClass::UnknownVariant,
                    codes::OUTPUT_UNEXPECTED,
                    format!("The operation `{}` found nothing to read.", found.name),
                    "Check the inputs; the record they name does not exist.",
                )));
            }
        }
    }
    outputs_of(loaded, found, &seen.values, true).map_err(failed)
}

/// A write tool: the preview, the approval check, reconcile on resume, and
/// the write path.
async fn write_tool(run: &mut Run<'_>, loaded: &Loaded, tool: &mut Tool<'_>) -> Result<Done, Stop> {
    let first_write = tool
        .steps
        .iter()
        .position(|step| step.writes)
        .unwrap_or(tool.steps.len());
    let all = tool.steps.len();
    let job = run.job;
    let previewed = job
        .snapshot
        .as_ref()
        .filter(|snapshot| snapshot.diff.is_some());
    let mut planned: Vec<Planned> = Vec::new();
    match previewed {
        None => {
            if run.job.kind == JobKind::Reconcile {
                return Ok(Done::Settled { remaining: 0 });
            }
            // The preview: the leading reads, then every record the write
            // steps would touch, step by step: looked up and classed. A
            // step all of whose records exist unchanged gives its result
            // to the steps after it.
            read_steps_of(run, loaded, tool, 0, first_write).await?;
            let mut diff = Diff {
                tool: tool.name.clone(),
                plan: loaded.hash,
                target: loaded.plan.header.target.clone(),
                environment: run.environment.clone(),
                identity: run.job.identity.clone(),
                records: Vec::new(),
            };
            let mut records = Vec::new();
            for position in first_write..all {
                if !tool.steps[position].writes {
                    continue;
                }
                let from = planned.len();
                derive_step(run, loaded, tool, position, &mut planned)?;
                for record in &mut planned[from..] {
                    let (diffed, row) = preview_record(run, loaded, record).await?;
                    diff.records.push(diffed);
                    records.push(row);
                }
                keep_step(tool, position, &planned);
            }
            let hash = diff.hash();
            let diff_blob = store(run, BlobKind::Diff, diff.encode()?).await?;
            run.control
                .store_preview(&Preview {
                    records,
                    diff: hash,
                    diff_blob,
                })
                .await?;
            if !diff.changes_anything() {
                // Nothing to create or update: no approval is needed, and
                // a second run of the same input changes nothing.
                run.moved(RunState::Running).await?;
            } else if run.job.kind == JobKind::Preview {
                return Ok(Done::NeedsApproval);
            } else {
                match run.control.approval(&hash).await? {
                    Clearance::Waits => return Ok(Done::NeedsApproval),
                    Clearance::Cleared => run.state = RunState::Running,
                }
            }
        }
        Some(snapshot) => {
            if run.job.kind == JobKind::Preview {
                return Ok(Done::NeedsApproval);
            }
            let reads_only = matches!(run.job.kind, JobKind::Reconcile | JobKind::VerifyAfterLoss);
            if !snapshot.cleared {
                if reads_only {
                    // Nothing was ever written, so nothing is in doubt.
                    let waiting = snapshot
                        .records
                        .iter()
                        .filter(|record| record.state == RecordState::Planned)
                        .count();
                    return Ok(Done::Settled {
                        remaining: u32::try_from(waiting).unwrap_or(u32::MAX),
                    });
                }
                // An execute begins by using the approval bound to the
                // diff, which moves the run to `running`.
                let diff = snapshot.diff.unwrap_or(loaded.hash);
                match run.control.approval(&diff).await? {
                    Clearance::Waits => return Ok(Done::NeedsApproval),
                    Clearance::Cleared => run.state = RunState::Running,
                }
            }
            // What the approval was given for is the diff whose hash the run
            // carries; a diff read back under another hash names nothing that
            // was approved.
            if snapshot
                .approved
                .as_ref()
                .is_some_and(|approved| Some(approved.hash()) != snapshot.diff)
            {
                return Err(Stop::Fail(stale_approval(
                    "The stored diff is not the one the run was approved on.",
                )));
            }
            // The leading reads run again: approval can take minutes, and
            // the diff was approved on what they showed.
            read_steps_of(run, loaded, tool, 0, first_write).await?;
            for position in first_write..all {
                if !tool.steps[position].writes {
                    continue;
                }
                let from = planned.len();
                derive_step(run, loaded, tool, position, &mut planned)?;
                adopt(
                    loaded,
                    &mut planned[from..],
                    &snapshot.records,
                    snapshot.approved.as_ref(),
                )?;
                // A record that was found unchanged has no outputs in the
                // ledger: it is read once more, for the steps after it and
                // for the tool's result. One that reconcile found had
                // landed has them, kept when it was settled.
                for record in &mut planned[from..] {
                    let found = operation(loaded, record.operation)?;
                    let settled = record.state == RecordState::Unchanged;
                    // A record one of whose stored values came back masked
                    // is not read by them.
                    if settled
                        && record.outputs.is_empty()
                        && !found.outputs.is_empty()
                        && !record.masked
                    {
                        run.checkpoint(Duration::ZERO)?;
                        let held = hold_for(run, found, record, None, false).await?;
                        if let Some(raw) = read_record(run, found, record, held, false).await? {
                            record.outputs = outputs_of(loaded, found, &raw, false)?;
                        }
                    }
                }
                keep_step(tool, position, &planned);
            }
            if planned.len() != snapshot.records.len() {
                return Err(Stop::Fail(stale(
                    "The run would no longer touch the records its approved diff holds.",
                )));
            }
            if run.job.kind == JobKind::VerifyAfterLoss {
                verify_again(run, loaded, &mut planned).await?;
                return Ok(Done::Settled {
                    remaining: remaining(&planned),
                });
            }
            if snapshot.cleared {
                // Reconcile: every record in doubt is read and settled
                // before anything is written.
                reconcile(run, loaded, &mut planned, &snapshot.records).await?;
                if run.job.kind == JobKind::Reconcile {
                    return Ok(Done::Settled {
                        remaining: remaining(&planned),
                    });
                }
                run.moved(RunState::Running).await?;
            }
        }
    }
    // The write path, record by record in the order of the steps.
    for index in 0..planned.len() {
        match planned[index].state {
            RecordState::Verified | RecordState::Unchanged => {}
            RecordState::Acted if planned[index].acknowledged => {}
            RecordState::Planned => {
                if planned[index].pending {
                    // The record waited for one written earlier in the
                    // run, whose result is known now.
                    resolve(loaded, tool, &mut planned[index])?;
                }
                // A record that fails stops the run there: the records
                // after it stay `planned`, and none of them is written.
                if let Err(stop) = write_record(run, loaded, &mut planned[index]).await {
                    if matches!(stop, Stop::Fail(_)) {
                        block_children(run, loaded, tool, &planned[index + 1..]).await;
                    }
                    return Err(stop);
                }
            }
            RecordState::Intent | RecordState::Acted | RecordState::Failed => {
                return Err(Stop::Fail(codes::fail(
                    FailureClass::PossibleCommit,
                    codes::CANNOT_RECONCILE,
                    "A record of the run is still in doubt.",
                    "Reconcile the run before it is resumed.",
                )));
            }
        }
        // The step's last record is done: its result is there for the
        // steps after it.
        let seq = planned[index].record.step_seq;
        if planned
            .get(index + 1)
            .is_none_or(|next| next.record.step_seq != seq)
            && let Some(position) = tool.steps.iter().position(|step| step.seq == seq)
        {
            keep_step(tool, position, &planned);
        }
    }
    // The read steps after the first write step run only now.
    read_steps_of(run, loaded, tool, first_write, all).await?;
    run.control
        .send(EngineEvent::Result(kept(run, &tool.last)))
        .await?;
    Ok(Done::Completed)
}

fn remaining(planned: &[Planned]) -> u32 {
    let count = planned
        .iter()
        .filter(|record| record.state == RecordState::Planned)
        .count();
    u32::try_from(count).unwrap_or(u32::MAX)
}

/// Gives a write step's result to the steps after it, once every one of its
/// records is verified or unchanged.
fn keep_step(tool: &mut Tool<'_>, position: usize, planned: &[Planned]) {
    let seq = tool.steps[position].seq;
    let mut results = Vec::new();
    for record in planned
        .iter()
        .filter(|record| record.record.step_seq == seq)
    {
        if !matches!(record.state, RecordState::Verified | RecordState::Unchanged)
            && !record.acknowledged
        {
            return;
        }
        results.push(record.outputs.clone());
    }
    if results.is_empty() {
        return;
    }
    let kept = StepShape {
        ..tool.steps[position]
    };
    tool.keep(&kept, results);
}

/// The records one write step would touch, from the tool's inputs, its
/// source, and the results of the steps before it.
fn derive_step(
    run: &Run<'_>,
    loaded: &Loaded,
    tool: &Tool<'_>,
    position: usize,
    planned: &mut Vec<Planned>,
) -> Result<(), Stop> {
    let step = &tool.steps[position];
    let found = operation(loaded, step.operation)?;
    let fields = compare::record_fields(loaded, found);
    // An input may wait only for a record that is still to be written.
    let may_wait = planned.iter().any(|record| {
        !matches!(record.state, RecordState::Verified | RecordState::Unchanged)
            && !record.acknowledged
    });
    let calls = tool.calls(loaded, step, may_wait)?;
    for (position, call) in calls.into_iter().enumerate() {
        let record = RecordRef {
            run: run.job.run,
            step_seq: step.seq,
            record_seq: u32::try_from(position).unwrap_or(u32::MAX),
        };
        let key = if call.pending {
            place_key(found, record)?
        } else {
            key_of(found, &call.inputs, record)?
        };
        let place_keyed = call.pending || found.identity.key.is_empty();
        if kept(run, &call.inputs) != call.inputs {
            // A value that holds a registered secret is masked wherever it
            // is stored, shown, or read back, so a record written with one
            // could never be compared with what the system holds. Rule 4
            // keeps secrets out of tool inputs; a write that carries one
            // is refused before anything is sent.
            return Err(Stop::Fail(codes::fail(
                FailureClass::Input,
                codes::INPUT_INVALID,
                format!(
                    "A value the operation `{}` would write holds a registered secret, and a write of one cannot be verified.",
                    found.name
                ),
                "Leave the secret out of the tool's input; a secret reaches a system only through its session.",
            )));
        }
        let intended = compare::intended(&fields, &call.inputs);
        let binding = if call.pending {
            // The binding is chosen when the record's inputs are whole.
            0
        } else {
            let scope = run.base.with_input(&call.inputs);
            let (candidates, _) = run.candidates(found, &scope)?;
            candidates.first().map_or(0, |candidate| candidate.binding)
        };
        one_save(found, found.bindings.get(binding))?;
        if run.posture != Posture::Live {
            // The sandbox writes only to declared test records.
            let environment = &loaded.plan.environments[run.env_position];
            if !environment.test_records.contains(&key) {
                return Err(Stop::Fail(not_a_test_record()));
            }
        }
        planned.push(Planned {
            record,
            operation: step.operation,
            digest: intended.digest(),
            inputs: call.inputs,
            key,
            fields: fields.clone(),
            intended,
            before: None,
            current: None,
            seen_at: None,
            seen_in: None,
            state: RecordState::Planned,
            stamp: None,
            binding,
            outputs: Values::new(),
            known: Values::new(),
            assigned: None,
            recorded: false,
            pending: call.pending,
            masked: false,
            place_keyed,
            recorded_values: Values::new(),
            acknowledged: false,
            approved: Vec::new(),
        });
    }
    Ok(())
}

/// Works out the inputs of a record that waited for one written earlier in
/// the run. What was approved does not change: the values it writes were
/// known at the preview, and only its key was not.
fn resolve(loaded: &Loaded, tool: &Tool<'_>, record: &mut Planned) -> Result<(), Stop> {
    let step = tool
        .steps
        .iter()
        .find(|step| step.seq == record.record.step_seq)
        .ok_or_else(|| codes::job_invalid("A record names a step the tool does not have."))?;
    let call = tool
        .calls(loaded, step, false)?
        .into_iter()
        .nth(usize::try_from(record.record.record_seq).unwrap_or(usize::MAX));
    let Some(call) = call else {
        return Err(Stop::Fail(stale(
            "The run would no longer touch the records its approved diff holds.",
        )));
    };
    if compare::intended(&record.fields, &call.inputs).digest() != record.digest {
        return Err(Stop::Fail(stale(
            "A record's intended values are no longer the values that were approved.",
        )));
    }
    record.inputs = call.inputs;
    record.pending = false;
    Ok(())
}

/// Marks the records that wait for a record that failed: they are not
/// written, and the ledger says why.
async fn block_children(run: &Run<'_>, loaded: &Loaded, tool: &Tool<'_>, later: &[Planned]) {
    let blocked = codes::fail(
        FailureClass::Busy,
        codes::BLOCKED_BY_PARENT,
        "The record waits for a record that failed earlier in the run.",
        "Settle the earlier record, then resume the run.",
    );
    for record in later
        .iter()
        .filter(|record| record.pending && record.state == RecordState::Planned)
    {
        let still_waits = tool
            .steps
            .iter()
            .find(|step| step.seq == record.record.step_seq)
            .and_then(|step| tool.calls(loaded, step, true).ok())
            .and_then(|calls| {
                calls
                    .get(usize::try_from(record.record.record_seq).unwrap_or(usize::MAX))
                    .map(|call| call.pending)
            })
            .unwrap_or(true);
        if still_waits {
            fail_record(run, record, &blocked, None).await;
        }
    }
}

/// Takes each record's state, what the ledger knows of it, and the values
/// seen before, from the ledger. A record the ledger planned with other
/// content means the input, the source, or a leading read no longer gives
/// what was approved.
fn adopt(
    loaded: &Loaded,
    planned: &mut [Planned],
    snapshot: &[RecordSnapshot],
    approved: Option<&Diff>,
) -> Result<(), Stop> {
    for record in planned {
        // The bindings the approved diff lets write the record. A record the
        // diff does not hold, or a run with no diff read back, has none, and
        // nothing writes it.
        if let Some(diffed) = approved.and_then(|diff| {
            diff.records.iter().find(|diffed| {
                diffed.step_seq == record.record.step_seq
                    && diffed.record_seq == record.record.record_seq
            })
        }) {
            record.approved = diffed.approved_bindings();
            if let Some(binding) = &diffed.binding {
                record.binding = usize::try_from(binding.position).unwrap_or(usize::MAX);
            }
        }
        let Some(held) = snapshot.iter().find(|held| held.record == record.record) else {
            return Err(Stop::Fail(stale(
                "The run would touch a record its approved diff does not hold.",
            )));
        };
        let found = operation(loaded, record.operation)?;
        if held.key != record.key {
            // A record planned while its key waited for an earlier record
            // keeps the key the ledger planned it under: its place in the
            // run.
            if held.key == place_key(found, record.record)? {
                record.key = held.key.clone();
                record.place_keyed = true;
            } else {
                return Err(Stop::Fail(stale(
                    "A record's key is no longer the key that was approved.",
                )));
            }
        }
        if held.intended != record.digest {
            return Err(Stop::Fail(stale(
                "A record's intended values are no longer the values that were approved.",
            )));
        }
        record.state = held.state;
        record.acknowledged = held.acknowledged;
        record.before.clone_from(&held.before);
        record.stamp = Some(held.idempotency_token.clone());
        if held.state == RecordState::Planned || held.state == RecordState::Unchanged {
            // Nothing was sent for it, so nothing is known beyond its
            // inputs.
            continue;
        }
        // What the ledger holds was stored through the redactor. A value it
        // masked is not what the system holds: it is left out, so that it
        // is never sent back to the system as an argument or a key, and the
        // record is marked.
        let mut known = Values::new();
        let stored = held
            .before_commit
            .iter()
            .chain(held.outputs.iter())
            .flat_map(|values| values.iter());
        for (name, value) in stored {
            if masked(value) {
                record.masked = true;
            } else {
                known.insert(name.to_owned(), value.clone());
            }
        }
        let assigned = held
            .assigned_key
            .as_deref()
            .filter(|text| !text.contains(REDACTED));
        if assigned.is_none() && held.assigned_key.is_some() {
            record.masked = true;
        }
        record.recorded = held.before_commit.is_some();
        record.recorded_values = held
            .before_commit
            .iter()
            .flat_map(|values| values.iter())
            .filter(|(_, value)| !masked(value))
            .map(|(name, value)| (name.to_owned(), value.clone()))
            .collect();
        record.assigned = assigned.and_then(parse_key);
        record.known = known;
        // A record that ended on the system's acknowledgement gave its
        // outputs with its save, and they were stored with it: the steps
        // after it take them, as they take a verified record's.
        if held.state == RecordState::Verified || held.acknowledged {
            record.outputs = outputs_of(loaded, found, &record.known, false)?;
        }
    }
    Ok(())
}

/// Chooses and prepares the binding that writes or reads a record. With
/// `writes`, the context is the one the record is written in, which differs
/// from the one it is read in only in the sandbox.
async fn hold_for(
    run: &mut Run<'_>,
    found: &OperationPlan,
    record: &mut Planned,
    mode: Option<&a2m_core::Mode>,
    writes: bool,
) -> Result<usize, Stop> {
    let scope = run.base.with_input(&record.inputs);
    let (candidates, passed_over) = run.candidates(found, &scope)?;
    // A record is never carried by two modes: a record whose intent was
    // stored for one mode is read, and written, through that mode.
    let candidate = match mode {
        Some(mode) => candidates
            .iter()
            .find(|candidate| candidate.handle.mode() == mode),
        None => candidates
            .iter()
            .find(|candidate| candidate.binding == record.binding)
            .or(candidates.first()),
    };
    let Some(candidate) = candidate else {
        return Err(Stop::Fail(codes::fail(
            FailureClass::Busy,
            codes::NO_BINDING,
            format!(
                "The binding that wrote a record of `{}` may not act now.",
                found.name
            ),
            "Make the binding's provider ready, then resume the run.",
        )));
    };
    record.binding = candidate.binding;
    run.chosen(&found.name, candidate, &passed_over).await;
    let binding = &found.bindings[candidate.binding];
    let held = run
        .hold(&found.name, binding, candidate, false, writes)
        .await?;
    Ok(held)
}

/// Reads a record as its identity strategy says (spec section 209).
/// `Ok(None)` when it does not exist. A sign that the session ended is
/// relayed to the keeper.
///
/// A record that has an assigned key, or values recorded before its commit
/// point, is read by the binding's read-back steps, which find those values
/// under `result` and the assigned key as the step's key. A `stamped` or
/// `find` record that has neither is looked up: by its stamp, or by the
/// declared search. What a lookup finds is the read, and a key it reports
/// is kept. With `settles`, a lookup that finds nothing waits out the
/// binding's settle time once and looks again, because a lookup may lag
/// behind the write it is asked about.
async fn read_record(
    run: &mut Run<'_>,
    found: &OperationPlan,
    record: &mut Planned,
    held: usize,
    settles: bool,
) -> Result<Option<Values>, Stop> {
    let binding = &found.bindings[record.binding];
    let searches = matches!(
        found.identity.strategy,
        IdentityStrategy::Stamped | IdentityStrategy::Find
    ) && !binding.lookup.is_empty()
        && record.assigned.is_none()
        && !record.recorded;
    let steps = if searches {
        &binding.lookup
    } else {
        &binding.read_back
    };
    if steps.is_empty() {
        return Err(Stop::Fail(codes::fail(
            FailureClass::UnknownVariant,
            codes::NO_READ_BACK,
            format!(
                "The binding of `{}` has no steps that read its record, so nothing it writes can be verified.",
                found.name
            ),
            "Give the binding a read-back and publish the target again.",
        )));
    }
    let mut waited = false;
    loop {
        match read_once(run, found, record, held, steps, searches).await? {
            Some(values) => return Ok(Some(values)),
            None => {
                if searches && settles && !waited && binding.settle_ms > 0 {
                    // Inside the settle time, nothing found means not
                    // known yet. The wait is the one the binding declares
                    // for its lookup, and it is made only when the lease
                    // covers it and the second look; otherwise the job
                    // ends with the record still in doubt.
                    waited = true;
                    let settle = Duration::from_millis(u64::from(binding.settle_ms));
                    if !run.job.lease.leaves(settle + run.time_of(steps)) {
                        return Err(Stop::LeaseEnding);
                    }
                    tokio::time::sleep(settle).await;
                    continue;
                }
                return Ok(None);
            }
        }
    }
}

/// One pass of a record's read steps: its read-back, or, with `searches`,
/// its lookup, whose finding is kept as what is known of the record.
/// `Ok(None)` when a step answered that the record is absent.
async fn read_once(
    run: &mut Run<'_>,
    found: &OperationPlan,
    record: &mut Planned,
    held: usize,
    steps: &[a2m_core::Step],
    searches: bool,
) -> Result<Option<Values>, Stop> {
    // A read is given the record's key in the system, never its place in
    // the run: a key is never guessed.
    let key = read_key(found, record);
    let stamp = record.stamp.clone();
    let ledger_key = record.key.clone();
    let recorded = record.recorded_values.clone();
    let mut scope = run.base.with_input(&record.inputs);
    scope.result = Value::Record(record.known.clone());
    scope.current = record.current.clone().map_or(Value::Null, Value::Record);
    scope.record = Scope::record_of(key.as_ref());
    let cx = RecordCx {
        record: record.record,
        operation: &found.name,
        ledger_key: &ledger_key,
        key: key.as_ref(),
        stamp: stamp.as_deref(),
        recorded: &recorded,
    };
    let read = run
        .read_steps(
            held,
            Phase::ReadBack,
            steps,
            &mut scope,
            Some(cx),
            &record.fields,
        )
        .await;
    match read {
        Ok(Some(observed)) => {
            if searches {
                for (name, value) in observed.values.iter() {
                    record.known.insert(name.to_owned(), value.clone());
                }
                if observed.assigned_key.is_some() {
                    record.assigned = observed.assigned_key;
                }
            }
            Ok(Some(observed.values))
        }
        Ok(None) => Ok(None),
        Err(failure) => {
            if failure.failure.class() == FailureClass::SessionExpired {
                run.report_expiry(held).await;
            } else {
                run.poison(held);
            }
            Err(Stop::Fail(failure.failure))
        }
    }
}

async fn store(run: &Run<'_>, kind: BlobKind, bytes: Vec<u8>) -> Result<BlobRef, Failure> {
    let class = match kind {
        BlobKind::Trace | BlobKind::Screenshot | BlobKind::Snapshot => RetentionClass::Short,
        _ => RetentionClass::Long,
    };
    run.control
        .evidence(Evidence { kind, class, bytes })
        .await
        .map_err(|failure| {
            codes::fail(
                FailureClass::Busy,
                codes::EVIDENCE_NOT_STORED,
                format!(
                    "Evidence could not be stored ({}), so nothing can be marked verified.",
                    failure.code()
                ),
                "Free space in the evidence store, then resume the run.",
            )
        })
}

/// The evidence of one read of a record: what the system gave, without the
/// fields labelled restricted and after redaction, and the values as they
/// were compared.
async fn read_evidence(
    run: &Run<'_>,
    record: &Planned,
    raw: Option<&Values>,
    seen: Option<&Values>,
) -> Result<BlobRef, Failure> {
    let shown: Values = raw
        .map(|raw| {
            record
                .fields
                .iter()
                .filter(|field| field.sensitivity != a2m_core::Sensitivity::Restricted)
                .filter_map(|field| {
                    raw.get(&field.name)
                        .map(|value| (field.name.clone(), value.clone()))
                })
                .collect()
        })
        .unwrap_or_default();
    let evidence = Values::new()
        .with("exists", raw.is_some())
        .with("raw", kept(run, &shown))
        .with(
            "normalized",
            seen.map(|seen| kept(run, seen)).unwrap_or_default(),
        )
        .with("intended", kept(run, &record.intended));
    store(run, BlobKind::Values, encode_values(&evidence)?).await
}

/// The preview of one record: look it up, class it, and give its part of
/// the diff and its row for the ledger.
async fn preview_record(
    run: &mut Run<'_>,
    loaded: &Loaded,
    record: &mut Planned,
) -> Result<(DiffRecord, PlannedRecord), Stop> {
    let found = operation(loaded, record.operation)?;
    run.checkpoint(Duration::ZERO)?;
    // Three kinds of record have no current value to look up, and are a
    // `create` in every diff: one that cannot be found again; one found by
    // its stamp, which no record carries before this run has written it;
    // and one whose key waits for a record that does not exist yet. A
    // fourth has a key and a binding with nothing to read it with: it is
    // not read, and the diff says that what it holds now cannot be read.
    let unreadable = !record.pending
        && found.identity.strategy != IdentityStrategy::None
        && found.bindings.get(record.binding).is_some_and(cannot_read);
    let looked_up = !record.pending
        && !unreadable
        && match found.identity.strategy {
            IdentityStrategy::None => false,
            IdentityStrategy::Stamped => record.stamp.is_some(),
            IdentityStrategy::Natural | IdentityStrategy::Composite | IdentityStrategy::Find => {
                true
            }
        };
    let raw = if looked_up {
        let held = hold_for(run, found, record, None, false).await?;
        let raw = read_record(run, found, record, held, false).await?;
        record.seen_in = Some(run.serial_of(held));
        raw
    } else {
        None
    };
    let seen = raw
        .as_ref()
        .map(|raw| compare::seen(loaded, &record.fields, &record.intended, raw))
        .transpose()?;
    let action = match &seen {
        None if unreadable => DiffAction::Write,
        None => DiffAction::Create,
        Some(seen) if *seen == record.intended => DiffAction::Unchanged,
        Some(_) => DiffAction::Update,
    };
    // The diff is stored, and shown to the person who approves it: every
    // value in it passes the redactor. Whether a field changes is worked
    // out on the values themselves.
    let fields = record
        .fields
        .iter()
        .filter_map(|field| {
            let proposed = record.intended.get(&field.name)?;
            let current = seen.as_ref().and_then(|seen| seen.get(&field.name));
            let restricted = field.sensitivity == a2m_core::Sensitivity::Restricted;
            let current_raw = raw
                .as_ref()
                .and_then(|raw| raw.get(&field.name))
                .filter(|_| !restricted)
                .map(|value| kept_value(run, value));
            Some(DiffField {
                changes: current != Some(proposed),
                name: field.name.clone(),
                current: current.map(|value| kept_value(run, value)),
                current_raw,
                proposed: kept_value(run, proposed),
            })
        })
        .collect();
    let intended_values = store(
        run,
        BlobKind::Values,
        encode_values(&kept(run, &record.intended))?,
    )
    .await?;
    // The values seen before the write are held as they are stored, here
    // and on resume, and every later read is compared with them in that
    // form.
    let seen_kept = seen.as_ref().map(|seen| kept(run, seen));
    let before = match &seen_kept {
        Some(seen) => Some(store(run, BlobKind::Values, encode_values(seen)?).await?),
        None => None,
    };
    // The binding that writes the record, as the person who approves the
    // diff is shown it. A record whose key waits for an earlier record has
    // its binding chosen only then: every binding it could take is named,
    // and its evidence is the weakest of theirs, so that the page warns of
    // a write that may not be read back.
    let named = |position: usize, binding: &a2m_core::BindingPlan| DiffBinding {
        position: u32::try_from(position).unwrap_or(u32::MAX),
        mode: binding.mode.clone(),
        evidence: binding.evidence,
    };
    let (binding, could_take) = if record.pending {
        let all: Vec<DiffBinding> = found
            .bindings
            .iter()
            .enumerate()
            .map(|(position, binding)| named(position, binding))
            .collect();
        (None, all)
    } else {
        (
            found
                .bindings
                .get(record.binding)
                .map(|binding| named(record.binding, binding)),
            Vec::new(),
        )
    };
    let evidence = binding
        .iter()
        .chain(&could_take)
        .map(|binding| binding.evidence)
        .min_by_key(|evidence| match evidence {
            WriteEvidence::Acknowledged => 0,
            WriteEvidence::Response => 1,
            WriteEvidence::ReadBack => 2,
        })
        .unwrap_or(WriteEvidence::ReadBack);
    record.approved = binding
        .iter()
        .chain(&could_take)
        .filter_map(|binding| usize::try_from(binding.position).ok())
        .collect();
    record.before = seen_kept;
    record.current = raw;
    record.seen_at = Some(run.engine.deps.clock.elapsed());
    if action == DiffAction::Unchanged {
        record.state = RecordState::Unchanged;
        if let Some(raw) = &record.current {
            record.outputs = outputs_of(loaded, found, raw, false)?;
        }
    }
    Ok((
        DiffRecord {
            step_seq: record.record.step_seq,
            record_seq: record.record.record_seq,
            operation: found.name.clone(),
            key: record.key.clone(),
            action,
            fields,
            evidence,
            binding,
            could_take,
        },
        PlannedRecord {
            step_seq: record.record.step_seq,
            record_seq: record.record.record_seq,
            key: record.key.clone(),
            operation: found.name.clone(),
            intended: record.digest,
            intended_values: Some(intended_values),
            before,
            unchanged: action == DiffAction::Unchanged,
        },
    ))
}

/// Reconcile: every record in doubt, in key order, is read and settled
/// before anything is written.
async fn reconcile(
    run: &mut Run<'_>,
    loaded: &Loaded,
    planned: &mut [Planned],
    snapshot: &[RecordSnapshot],
) -> Result<(), Stop> {
    let mut doubtful: Vec<usize> = Vec::new();
    for (position, record) in planned.iter_mut().enumerate() {
        let held = snapshot.iter().find(|held| held.record == record.record);
        match (record.state, held.and_then(|held| held.failure.as_ref())) {
            // Ended on the system's acknowledgement: settled, never read.
            (RecordState::Acted, _) if record.acknowledged => {}
            (RecordState::Intent | RecordState::Acted, _) => doubtful.push(position),
            (RecordState::Failed, Some((FailureClass::Busy, code)))
                if *code == codes::BLOCKED_BY_PARENT
                    && held.is_some_and(|held| {
                        held.mode.is_none()
                            && held.outputs.is_none()
                            && held.assigned_key.is_none()
                            && held.before_commit.is_none()
                    }) =>
            {
                // Nothing was ever sent for a record that waited for its
                // parent: it goes back to `planned` without a read. The
                // code alone does not say so, because a provider may use
                // the same name: the record must also never have had an
                // intent, and nothing of it may be known. Any other record
                // that failed with this code is read like every record in
                // doubt. The ledger checks from its own row that no intent
                // was stored for it.
                run.control.lock(record.record).await?;
                match run
                    .control
                    .settle(record.record, Reconciled::NeverSent)
                    .await?
                {
                    Settled::Planned => record.state = RecordState::Planned,
                    Settled::Verified | Settled::Stale => {
                        return Err(Stop::Fail(codes::fail(
                            FailureClass::PossibleCommit,
                            codes::CANNOT_RECONCILE,
                            "A record that waited for an earlier record was not returned to `planned`.",
                            "Look at the run's records, then cancel the run or settle the record by hand.",
                        )));
                    }
                }
            }
            (RecordState::Failed, Some((class, code))) if settles(*class) => {
                // Nothing is in doubt and nothing can go on: the record
                // was read, and it is not what was approved.
                return Err(Stop::Fail(Failure::new(
                    *class,
                    code.clone(),
                    "A record of the run failed and cannot be written under the approved diff.",
                    "Preview the run again and approve the new diff.",
                )));
            }
            (RecordState::Failed, _) => doubtful.push(position),
            _ => {}
        }
    }
    doubtful.sort_by_key(|position| planned[*position].key.canonical_bytes());
    for position in doubtful {
        let record = &mut planned[position];
        let held = snapshot.iter().find(|held| held.record == record.record);
        let found = operation(loaded, record.operation)?;
        let mode = held.and_then(|held| held.mode.clone());
        if found.identity.strategy == IdentityStrategy::None
            || found.bindings.get(record.binding).is_some_and(cannot_read)
        {
            // A record of no identity has no key to be read by, and a
            // record whose binding has nothing to read it with cannot be
            // read by its key: each is settled only as `reconcile_keyless`
            // says (decisions 0185 and 0196), by a lookup the pack stands
            // behind for absence, and a found record ended on an
            // acknowledgement stays in doubt.
            reconcile_keyless(run, loaded, found, record, held).await?;
            continue;
        }
        // A failure of a class that would end the run for good says only
        // that the record could not be read: it is still in doubt, and the
        // run stops for a resume or a person.
        let (raw, seen, evidence, context) = match read_in_doubt(
            run,
            loaded,
            found,
            record,
            mode.as_ref(),
        )
        .await
        {
            Err(Stop::Fail(failure)) if settles(failure.class()) => {
                return Err(Stop::Fail(codes::fail(
                    FailureClass::PossibleCommit,
                    codes::CANNOT_RECONCILE,
                    format!(
                        "A record of `{}` may have been written and could not be read (`{}/{}`).",
                        found.name,
                        failure.class(),
                        failure.code()
                    ),
                    "Look at the system, then resume the run, cancel it, or settle the record by hand.",
                )));
            }
            read => read?,
        };
        let seen_kept = seen.as_ref().map(|seen| kept(run, seen));
        // What is known of the record with what the read gave: its outputs
        // when it landed.
        let mut observed = record.known.clone();
        for (name, value) in raw.clone().unwrap_or_default() {
            observed.insert(name, value);
        }
        let reconciled = match &seen {
            Some(seen) if *seen == record.intended => {
                let after = store(
                    run,
                    BlobKind::Values,
                    encode_values(&seen_kept.clone().unwrap_or_default())?,
                )
                .await?;
                // The outputs and the assigned key are kept with the record,
                // as on the normal path, so that a later resume has them
                // without reading the record once more (decision 0145).
                let outputs = if observed.is_empty() {
                    None
                } else {
                    Some(store(run, BlobKind::Values, encode_values(&kept(run, &observed))?).await?)
                };
                let assigned_key = record
                    .assigned
                    .as_ref()
                    .map(|key| run.engine.deps.redact.redact(&key_text(key)));
                Reconciled::Landed {
                    read: ReadBack {
                        digest: seen.digest(),
                        after: Some(after),
                        evidence,
                    },
                    acted: Acted {
                        assigned_key,
                        outputs,
                    },
                }
            }
            _ if seen_kept == record.before => Reconciled::NotLanded { evidence },
            _ => Reconciled::Neither { evidence },
        };
        match run.control.settle(record.record, reconciled).await? {
            Settled::Verified => {
                record.state = RecordState::Verified;
                record.outputs = outputs_of(loaded, found, &observed, false)?;
            }
            Settled::Planned => {
                // Nothing landed, so what was recorded for the lost write
                // names nothing, and the next attempt records its own.
                record.state = RecordState::Planned;
                record.known = Values::new();
                record.assigned = None;
                record.recorded = false;
                record.recorded_values = Values::new();
                record.current = raw;
                record.seen_at = Some(run.engine.deps.clock.elapsed());
                record.seen_in = Some(run.serial_of(context));
            }
            Settled::Stale => {
                return Err(Stop::Fail(stale(
                    "A record in doubt holds neither the values intended nor the values seen before the write.",
                )));
            }
        }
    }
    Ok(())
}

/// What a record of no identity in doubt is left as: `cannot_reconcile`,
/// with what was recorded before its save and, when a look found it, what
/// was found, shown as the system's data. Nothing is sent.
fn keyless_in_doubt(
    run: &Run<'_>,
    found: &OperationPlan,
    record: &Planned,
    why: &str,
    seen: Option<&Values>,
) -> Stop {
    let mut failure = codes::fail(
        FailureClass::PossibleCommit,
        codes::CANNOT_RECONCILE,
        format!(
            "A record of `{}` may have been written, and {why}.",
            found.name
        ),
        "Look at the system with what was recorded before the save, then resume the run, cancel it, or settle the record by hand.",
    );
    let mut said = Vec::new();
    if let Ok(bytes) = encode_values(&kept(run, &record.recorded_values)) {
        said.push(format!(
            "recorded before the save: {}",
            String::from_utf8_lossy(&bytes)
        ));
    }
    if let Some(seen) = seen
        && let Ok(bytes) = encode_values(&kept(run, seen))
    {
        said.push(format!("found: {}", String::from_utf8_lossy(&bytes)));
    }
    if !said.is_empty() {
        failure = failure.with_said(said.join("; "));
    }
    Stop::Fail(failure)
}

/// Settles a record of no identity that is in doubt (decision 0185). It has
/// no key, so "absent" is never proof by itself:
///
/// - a record whose recorded values were masked is not read at all, as
///   `read_in_doubt` refuses the same case;
/// - its binding's read-back, given what the record's earlier steps
///   recorded, may find it, and then what it found is compared with the
///   intended values as any read-back is; what the read-back does not find
///   proves nothing;
/// - it goes back to `planned` only when its binding's lookup is one the
///   mode's manifest states is authoritative for absence, asked once the
///   binding's settle time has passed since the send was recorded, answers
///   "absent", and answers it again after another settle time.
///
/// In every other case the record stays in doubt with `cannot_reconcile`,
/// showing what was recorded, and nothing is sent.
async fn reconcile_keyless(
    run: &mut Run<'_>,
    loaded: &Loaded,
    found: &OperationPlan,
    record: &mut Planned,
    held: Option<&RecordSnapshot>,
) -> Result<(), Stop> {
    run.checkpoint(Duration::ZERO)?;
    if record.masked {
        // What would find the record again was masked when it was stored.
        // Reading without it could find nothing, and nothing found must not
        // be taken for a write that did not land.
        return Err(keyless_in_doubt(
            run,
            found,
            record,
            "what would find it again holds a registered secret and was not stored",
            None,
        ));
    }
    let mode = held.and_then(|held| held.mode.clone());
    run.control.lock(record.record).await?;
    let context = hold_for(run, found, record, mode.as_ref(), false).await?;
    let binding = &found.bindings[record.binding];
    let unanswered = |failure: &Failure| {
        format!(
            "the look for it did not answer (`{}/{}`)",
            failure.class(),
            failure.code()
        )
    };
    if !binding.read_back.is_empty() && !record.recorded_values.is_empty() {
        match read_once(run, found, record, context, &binding.read_back, false).await {
            Ok(Some(raw)) => return keyless_found(run, loaded, found, record, raw).await,
            // A read-back that finds nothing may lag, scan a window, or read
            // one page: it proves nothing.
            Ok(None) => {}
            Err(Stop::Fail(failure)) => {
                return Err(keyless_in_doubt(
                    run,
                    found,
                    record,
                    &unanswered(&failure),
                    None,
                ));
            }
            Err(stop) => return Err(stop),
        }
    }
    if !run.proves_absence(context, &binding.lookup) {
        return Err(keyless_in_doubt(
            run,
            found,
            record,
            "nothing its binding declares can prove that it did not land",
            None,
        ));
    }
    // The lookup is asked only once the settle time has passed since the
    // send was recorded. Where the ledger recorded nothing after the
    // intent, the send was made, if at all, within the time the binding's
    // act steps may take after it.
    let settle_ms = i64::from(binding.settle_ms);
    let act_ms = i64::try_from(run.time_of(&binding.act).as_millis()).unwrap_or(i64::MAX);
    let sent_by = held.and_then(|held| {
        held.sent_by.map(Timestamp::as_unix_millis).or_else(|| {
            held.intent_at
                .map(|at| at.as_unix_millis().saturating_add(act_ms))
        })
    });
    let Some(sent_by) = sent_by else {
        return Err(keyless_in_doubt(
            run,
            found,
            record,
            "the ledger holds no time for its send, so its settle time cannot be counted",
            None,
        ));
    };
    let now = run.engine.deps.clock.now().as_unix_millis();
    let first = Duration::from_millis(
        u64::try_from(sent_by.saturating_add(settle_ms).saturating_sub(now)).unwrap_or(0),
    );
    let settle = Duration::from_millis(u64::from(binding.settle_ms));
    let looks = run.time_of(&binding.lookup);
    if !run.job.lease.leaves(first + settle + looks + looks) {
        // The lease does not cover both looks and the waits: the job ends
        // with the record still in doubt.
        return Err(Stop::LeaseEnding);
    }
    tokio::time::sleep(first).await;
    for look in 0..2 {
        if look == 1 {
            tokio::time::sleep(settle).await;
        }
        match read_once(run, found, record, context, &binding.lookup, true).await {
            Ok(Some(raw)) => return keyless_found(run, loaded, found, record, raw).await,
            Ok(None) => {}
            Err(Stop::Fail(failure)) => {
                return Err(keyless_in_doubt(
                    run,
                    found,
                    record,
                    &unanswered(&failure),
                    None,
                ));
            }
            Err(stop) => return Err(stop),
        }
    }
    let evidence = read_evidence(run, record, None, None).await?;
    match run
        .control
        .settle(record.record, Reconciled::NotLanded { evidence })
        .await?
    {
        Settled::Planned => {
            record.state = RecordState::Planned;
            record.known = Values::new();
            record.assigned = None;
            record.recorded = false;
            record.recorded_values = Values::new();
            record.current = None;
            record.seen_at = Some(run.engine.deps.clock.elapsed());
            record.seen_in = Some(run.serial_of(context));
            Ok(())
        }
        Settled::Verified | Settled::Stale => Err(keyless_in_doubt(
            run,
            found,
            record,
            "the ledger did not return it to `planned`",
            None,
        )),
    }
}

/// A record of no identity in doubt that a look found: what was found is
/// compared, field by field after normalization, with the intended values,
/// as every read-back is (rule 2). Equal settles it as verified, on the
/// digest of what was read. Different is a mismatch: never verified, never
/// sent again, and left in doubt for a person. A binding whose evidence is
/// the acknowledgement alone is not ended here, because the ledger ends a
/// record on an acknowledgement only under the lease that sent it
/// (decision 0172): it stays in doubt with what was found shown.
async fn keyless_found(
    run: &mut Run<'_>,
    loaded: &Loaded,
    found: &OperationPlan,
    record: &mut Planned,
    raw: Values,
) -> Result<(), Stop> {
    let acknowledged = found
        .bindings
        .get(record.binding)
        .is_some_and(|binding| binding.evidence == WriteEvidence::Acknowledged);
    if acknowledged {
        return Err(keyless_in_doubt(
            run,
            found,
            record,
            "a look found it, and a write that ends on the system's acknowledgement is ended only under the lease that sent it",
            Some(&raw),
        ));
    }
    let seen = match compare::seen(loaded, &record.fields, &record.intended, &raw) {
        Ok(seen) => seen,
        Err(failure) => {
            return Err(keyless_in_doubt(
                run,
                found,
                record,
                &format!(
                    "what a look found could not be compared (`{}/{}`)",
                    failure.class(),
                    failure.code()
                ),
                None,
            ));
        }
    };
    let evidence = read_evidence(run, record, Some(&raw), Some(&seen)).await?;
    if seen != record.intended {
        let differing = match compare::compare(&record.fields, &record.intended, &seen) {
            compare::Comparison::Differs(names) => names.join(", "),
            compare::Comparison::Equal => String::new(),
        };
        return Err(Stop::Fail(codes::fail(
            FailureClass::Mismatch,
            codes::READ_BACK_DIFFERS,
            format!(
                "A record of `{}` that was in doubt was found, and differs from what was intended: {differing}.",
                found.name
            ),
            "Compare the values in the evidence, then cancel the run or settle the record by hand; nothing sends it again.",
        )));
    }
    let after = store(run, BlobKind::Values, encode_values(&kept(run, &seen))?).await?;
    let mut observed = record.known.clone();
    for (name, value) in raw {
        observed.insert(name, value);
    }
    let outputs = if observed.is_empty() {
        None
    } else {
        Some(store(run, BlobKind::Values, encode_values(&kept(run, &observed))?).await?)
    };
    let reconciled = Reconciled::Landed {
        read: ReadBack {
            digest: seen.digest(),
            after: Some(after),
            evidence,
        },
        acted: Acted {
            assigned_key: record
                .assigned
                .as_ref()
                .map(|key| run.engine.deps.redact.redact(&key_text(key))),
            outputs,
        },
    };
    match run.control.settle(record.record, reconciled).await? {
        Settled::Verified => {
            record.state = RecordState::Verified;
            record.outputs = outputs_of(loaded, found, &observed, false)?;
            Ok(())
        }
        Settled::Planned | Settled::Stale => Err(keyless_in_doubt(
            run,
            found,
            record,
            "the ledger did not settle it as found",
            Some(&seen),
        )),
    }
}

/// Reads one record in doubt, through the mode its intent was stored for:
/// what the system gave, the values as they are compared, the evidence of
/// the read, and the context it was read in.
async fn read_in_doubt(
    run: &mut Run<'_>,
    loaded: &Loaded,
    found: &OperationPlan,
    record: &mut Planned,
    mode: Option<&a2m_core::Mode>,
) -> Result<(Option<Values>, Option<Values>, BlobRef, usize), Stop> {
    run.checkpoint(Duration::ZERO)?;
    // Take or confirm the lock under this lease, then read.
    run.control.lock(record.record).await?;
    let context = hold_for(run, found, record, mode, false).await?;
    let binding = &found.bindings[record.binding];
    let by_lookup = matches!(
        found.identity.strategy,
        IdentityStrategy::Stamped | IdentityStrategy::Find
    ) && !binding.lookup.is_empty()
        && record.assigned.is_none()
        && !record.recorded;
    if record.masked && !by_lookup {
        // What would find the record again was masked when it was stored.
        // Reading without it could find nothing, and nothing found would
        // be taken for a write that did not land.
        return Err(Stop::Fail(codes::fail(
            FailureClass::PossibleCommit,
            codes::CANNOT_RECONCILE,
            format!(
                "A record of `{}` may have been written, and what would find it again holds a registered secret and was not stored.",
                found.name
            ),
            "Look at the system, then cancel the run or settle the record by hand.",
        )));
    }
    let raw = read_record(run, found, record, context, true).await?;
    let seen = raw
        .as_ref()
        .map(|raw| compare::seen(loaded, &record.fields, &record.intended, raw))
        .transpose()?;
    let evidence = read_evidence(run, record, raw.as_ref(), seen.as_ref()).await?;
    Ok((raw, seen, evidence, context))
}

/// Reads every record the ledger holds as verified once more, and reports
/// each that no longer reads as it was written. It changes no state.
async fn verify_again(
    run: &mut Run<'_>,
    loaded: &Loaded,
    planned: &mut [Planned],
) -> Result<(), Stop> {
    for record in planned
        .iter_mut()
        .filter(|record| record.state == RecordState::Verified)
    {
        let found = operation(loaded, record.operation)?;
        if found.identity.strategy == IdentityStrategy::None {
            continue;
        }
        run.checkpoint(Duration::ZERO)?;
        let held = hold_for(run, found, record, None, false).await?;
        let raw = read_record(run, found, record, held, false).await?;
        let seen = raw
            .as_ref()
            .map(|raw| compare::seen(loaded, &record.fields, &record.intended, raw))
            .transpose()?;
        if seen.as_ref() != Some(&record.intended) {
            let evidence = read_evidence(run, record, raw.as_ref(), seen.as_ref()).await?;
            run.tell(EngineEvent::VerifiedDiffers {
                record: record.record,
                evidence,
            })
            .await;
        }
    }
    Ok(())
}

/// Marks a record failed. When that cannot be stored the record stays as
/// it was, in doubt, which is what a resume reads first.
async fn fail_record(run: &Run<'_>, record: &Planned, why: &Failure, evidence: Option<BlobRef>) {
    let _ = run.control.failed(record.record, why, evidence).await;
}

/// The write path for one record: lock, freshness, intent, act, confirm,
/// read back, record.
///
/// One rule is held here, whatever a step or the control plane reports:
/// from the moment the committing step was sent until the record is
/// verified, no failure settles the record as not written. A failure of a
/// class that would is the write in doubt: the ledger keeps the lock, the
/// run stops, and reconcile reads the record before anything is written
/// again.
async fn write_record(
    run: &mut Run<'_>,
    loaded: &Loaded,
    record: &mut Planned,
) -> Result<(), Stop> {
    let mut sent = false;
    match commit_point(run, loaded, record, &mut sent).await {
        Err(Stop::Fail(failure)) if sent && settles(failure.class()) => {
            Err(Stop::Fail(in_doubt("what followed", &failure)))
        }
        ended => ended,
    }
}

/// One commit point. `sent` is set once the committing step was sent and
/// the system did not refuse it.
async fn commit_point(
    run: &mut Run<'_>,
    loaded: &Loaded,
    record: &mut Planned,
    sent: &mut bool,
) -> Result<(), Stop> {
    let found = operation(loaded, record.operation)?;
    if run.posture != Posture::Live {
        // The sandbox writes only to a declared test record. The key is
        // checked here once more, where the write is made, and not only
        // where the records were worked out.
        let environment = &loaded.plan.environments[run.env_position];
        if !environment.test_records.contains(&record.key) {
            return Err(Stop::Fail(not_a_test_record()));
        }
    }
    run.checkpoint(Duration::ZERO)?;
    // The context the record is written in. Live it is the one every step
    // of the mode runs in. In the sandbox it is prepared for the write of
    // a declared test record alone: every other step of the job, the
    // preview's lookups among them, runs with the driver's own read-only
    // guard on.
    let held = hold_for(run, found, record, None, true).await?;
    if !record.approved.contains(&record.binding) {
        // The person approved the write through another binding, and was
        // shown that binding's evidence: nothing is sent through this one.
        return Err(Stop::Fail(stale_approval(&format!(
            "The binding that would write a record of `{}` now is not one the approved diff names for it.",
            found.name
        ))));
    }
    let binding = &found.bindings[record.binding];
    // The binding chosen now may not be the one the records were worked
    // out with: it is checked for its one save before anything is sent.
    one_save(found, Some(binding))?;
    if binding.evidence == WriteEvidence::ReadBack && binding.read_back.is_empty() {
        return Err(Stop::Fail(codes::fail(
            FailureClass::UnknownVariant,
            codes::NO_READ_BACK,
            format!(
                "The binding of `{}` has no read-back, so nothing it writes can be verified.",
                found.name
            ),
            "Give the binding a read-back and publish the target again.",
        )));
    }
    if binding.evidence == WriteEvidence::Acknowledged && found.effect != EffectClass::Irreversible
    {
        return Err(Stop::Fail(codes::fail(
            FailureClass::UnknownVariant,
            codes::NO_READ_BACK,
            format!(
                "The binding of `{}` offers only the system's acknowledgement, and the operation is not irreversible.",
                found.name
            ),
            "Declare the operation `irreversible`, or give the binding a read-back, and publish the target again.",
        )));
    }
    // A record found by its key and read back by the binding is verified by
    // that read, never ended on the answer alone (decision 0193). The plan
    // check refuses such a binding; a plan that reached the engine some
    // other way is held to it here, before anything is sent.
    if binding.evidence == WriteEvidence::Acknowledged
        && !a2m_core::acknowledgement_may_stand(found.identity.strategy, &binding.read_back)
    {
        return Err(Stop::Fail(codes::fail(
            FailureClass::UnknownVariant,
            codes::NO_READ_BACK,
            format!(
                "The binding of `{}` reads its record back by its key and offers the system's acknowledgement as its evidence; the read-back is what verifies it.",
                found.name
            ),
            "Make the binding's evidence its read-back, or drop the read-back, and publish the target again.",
        )));
    }
    // Whether the lease leaves time for the record's steps, each counted at
    // its real time limit. Asked before the waits, so that a job that
    // cannot finish the record does not queue for it.
    let whole = run.time_of(steps_of(binding));
    run.checkpoint(whole)?;
    let steps = binding.act.len() + binding.confirm.len() + binding.read_back.len();
    run.budget(held, steps).await?;

    // Intend: the lock, the freshness check, then the intent.
    run.control.lock(record.record).await?;
    let settings = &run.engine.deps.settings;
    let now = run.engine.deps.clock.elapsed();
    // A read is fresh when it is recent and was made in the context that
    // writes. A driver may keep what it read of a record, such as its
    // version, with its context: a context prepared since the read, in
    // place of one that failed or for the sandbox's write, knows nothing
    // of the record, so the record is read again in it.
    let fresh = !run.job.conditions.strict_freshness
        && record
            .seen_at
            .is_some_and(|at| now.saturating_sub(at) <= settings.freshness_window)
        && record
            .seen_in
            .is_none_or(|context| context == run.serial_of(held));
    if !fresh && found.identity.strategy != IdentityStrategy::None && !cannot_read(binding) {
        let raw = read_record(run, found, record, held, false).await?;
        let seen = raw
            .as_ref()
            .map(|raw| compare::seen(loaded, &record.fields, &record.intended, raw))
            .transpose()?;
        if seen.as_ref().map(|seen| kept(run, seen)) != record.before {
            let failure = stale("A record no longer holds what the preview saw.");
            let evidence = read_evidence(run, record, raw.as_ref(), seen.as_ref())
                .await
                .ok();
            fail_record(run, record, &failure, evidence).await;
            return Err(Stop::Fail(failure));
        }
        record.current = raw;
        record.seen_at = Some(run.engine.deps.clock.elapsed());
        record.seen_in = Some(run.serial_of(held));
    }
    // The waits are over, and they may have used the lease up: the
    // directive and the lease are checked again before the intent.
    run.checkpoint(whole)?;
    // No ledger, no write: when the intent cannot be stored, nothing is
    // sent.
    let mut tokens = run
        .control
        .intend(vec![RecordIntent {
            record: record.record,
            intended: record.digest,
            mode: Some(run.mode_of(held)),
        }])
        .await?;
    let Some(token) = tokens.pop() else {
        return Err(Stop::Fail(codes::fail(
            FailureClass::Policy,
            codes::NO_TOKEN,
            "The ledger stored an intent and gave no token for it.",
            "Report this; nothing was sent.",
        )));
    };
    // The token carries the record's stamp: the steps that write and that
    // read the record are given it from here on.
    let stamp = token.idempotency_token().to_owned();
    record.stamp = Some(stamp.clone());
    let ledger_key = record.key.clone();
    let work_key = act_key(record).cloned();

    // Act.
    let mut scope = run.base.with_input(&record.inputs);
    scope.current = record.current.clone().map_or(Value::Null, Value::Record);
    let mut all = Observed::default();
    let mut token = Some(token);
    let mut acted: Option<ActedToken> = None;
    for (position, step) in binding.act.iter().enumerate() {
        // What the steps after this one may take, each at its time limit.
        let then = run.time_of(
            binding.act[position + 1..]
                .iter()
                .chain(&binding.confirm)
                .chain(&binding.read_back),
        );
        if step.commits {
            // The commit point. The lease is checked immediately before
            // the save is sent, against the save's own time limit and the
            // steps that verify it: a save is not sent unless the lease
            // covers it. The record stays in `intent` with nothing sent,
            // which the next lease reads first.
            if !run.job.lease.leaves(run.limit_of(step) + then) {
                return Err(Stop::LeaseEnding);
            }
            if !all.for_reconcile.is_empty() {
                // What reconcile will need is durable before the write is
                // sent.
                let stored = async {
                    let outputs = store(
                        run,
                        BlobKind::Values,
                        encode_values(&kept(run, &all.for_reconcile))?,
                    )
                    .await?;
                    run.control
                        .send(EngineEvent::BeforeCommit {
                            record: record.record,
                            outputs,
                        })
                        .await
                }
                .await;
                if let Err(failure) = stored {
                    fail_record(run, record, &failure, None).await;
                    return Err(Stop::Fail(failure));
                }
            }
        }
        let recorded = recorded_with(record, &all.for_reconcile);
        scope.record = Scope::record_of(work_key.as_ref());
        let cx = RecordCx {
            record: record.record,
            operation: &found.name,
            ledger_key: &ledger_key,
            key: work_key.as_ref(),
            stamp: Some(stamp.as_str()),
            recorded: &recorded,
        };
        let outcome = run
            .step(StepCx {
                held,
                phase: Phase::Act,
                position,
                step,
                scope: &scope,
                record: Some(cx),
                token: if step.commits { token.as_ref() } else { None },
                fields: &record.fields,
                then,
            })
            .await;
        let observed = match outcome {
            Ok(None) if step.commits => {
                let failure = codes::fail(
                    FailureClass::UnknownVariant,
                    codes::COMMIT_SKIPPED,
                    "The step that saves the record was skipped by its condition, so nothing was written.",
                    "Correct the step's condition and publish the target again.",
                );
                fail_record(run, record, &failure, None).await;
                return Err(Stop::Fail(failure));
            }
            Ok(None) => continue,
            Ok(Some(StepOutcome::Done(observed) | StepOutcome::Absent(observed))) => observed,
            Err(failure) => {
                if failure.failure.class() == FailureClass::SessionExpired {
                    run.report_expiry(held).await;
                } else {
                    run.poison(held);
                }
                let failure = if *sent {
                    // A step after the save.
                    after_send(failure.failure, "a step after the save")
                } else if step.commits {
                    commit_failure(failure)
                } else {
                    // A step before the save: nothing was sent.
                    failure.failure
                };
                fail_record(run, record, &failure, None).await;
                return Err(Stop::Fail(failure));
            }
        };
        if step.commits {
            // The write was sent and answered: from here until the record
            // is verified it is in doubt. Mark `acted` at once, with any
            // key the system assigned, before or with the save, and
            // everything the record's steps have observed so far.
            *sent = true;
            let Some(spent) = token.take() else {
                // A second save in one call is refused before anything is
                // sent; if one is met all the same, it is not sent.
                let failure = in_doubt(
                    "a second step that saves",
                    &codes::fail(
                        FailureClass::Busy,
                        codes::JOB_KIND_NOT_BUILT,
                        "The binding saves more than once in one call.",
                        "Split the operation into one operation for each save.",
                    ),
                );
                fail_record(run, record, &failure, None).await;
                return Err(Stop::Fail(failure));
            };
            let assigned_key = observed
                .assigned_key
                .as_ref()
                .or(all.assigned_key.as_ref())
                .map(|key| run.engine.deps.redact.redact(&key_text(key)));
            let mut observed_so_far = all.values.clone();
            for (name, value) in all.for_reconcile.iter().chain(observed.values.iter()) {
                observed_so_far.insert(name.to_owned(), value.clone());
            }
            // The record's outputs are stored with its `acted` mark, and
            // the ledger holds them from there only. When they cannot be
            // stored the record is still marked, so that the ledger says a
            // write was sent, and the job stops: nothing is dropped
            // silently, and a resume reads the record again.
            let stored = if observed_so_far.is_empty() {
                Ok(None)
            } else {
                match encode_values(&kept(run, &observed_so_far)) {
                    Ok(bytes) => store(run, BlobKind::Values, bytes).await.map(Some),
                    Err(failure) => Err(failure),
                }
            };
            let (outputs, not_stored) = match stored {
                Ok(outputs) => (outputs, None),
                Err(failure) => (None, Some(failure)),
            };
            match run
                .control
                .acted(
                    spent,
                    Acted {
                        assigned_key,
                        outputs,
                    },
                )
                .await
            {
                Ok(proof) => acted = Some(proof),
                Err(failure) => {
                    // The write was sent and could not be recorded.
                    return Err(Stop::Fail(codes::fail(
                        FailureClass::PossibleCommit,
                        codes::NO_CONFIRMATION,
                        format!(
                            "A write was sent and could not be recorded ({}).",
                            failure.code()
                        ),
                        "Reconcile the run: read the record back before anything is written again.",
                    )));
                }
            }
            if let Some(failure) = not_stored {
                return Err(Stop::Fail(failure));
            }
        }
        merge(&mut scope, &mut all, observed);
    }
    // What the steps learned is how the record is read from here on: by
    // the key the system assigned, and by the values recorded before the
    // commit point.
    let mut known = all.values.clone();
    for (name, value) in all.for_reconcile.iter() {
        known.insert(name.to_owned(), value.clone());
    }
    record.known = known;
    record.assigned.clone_from(&all.assigned_key);
    record.recorded = !all.for_reconcile.is_empty();
    record.recorded_values = recorded_with(record, &all.for_reconcile);
    let read_by = read_key(found, record);
    let recorded = record.recorded_values.clone();
    let cx = RecordCx {
        record: record.record,
        operation: &found.name,
        ledger_key: &ledger_key,
        key: read_by.as_ref(),
        stamp: Some(stamp.as_str()),
        recorded: &recorded,
    };
    scope.record = Scope::record_of(read_by.as_ref());
    let Some(acted) = acted else {
        let failure = codes::fail(
            FailureClass::UnknownVariant,
            codes::COMMIT_SKIPPED,
            "The binding ended without sending its write.",
            "Correct the binding and publish the target again.",
        );
        fail_record(run, record, &failure, None).await;
        return Err(Stop::Fail(failure));
    };

    // Confirm.
    let confirmed = run
        .read_steps(
            held,
            Phase::Confirm,
            &binding.confirm,
            &mut scope,
            Some(cx),
            &record.fields,
        )
        .await;
    match confirmed {
        Ok(Some(observed)) => {
            let mut into = Scope::new(Value::Null);
            merge(&mut into, &mut all, observed);
        }
        Ok(None) => {
            let failure = codes::fail(
                FailureClass::PossibleCommit,
                codes::NO_CONFIRMATION,
                "The system did not confirm the write.",
                "Reconcile the run: read the record back before anything is written again.",
            );
            fail_record(run, record, &failure, None).await;
            return Err(Stop::Fail(failure));
        }
        Err(failure) => {
            if failure.failure.class() == FailureClass::SessionExpired {
                run.report_expiry(held).await;
            } else {
                run.poison(held);
            }
            let failure = confirm_failure(failure);
            fail_record(run, record, &failure, None).await;
            return Err(Stop::Fail(failure));
        }
    }

    if binding.evidence == WriteEvidence::Acknowledged {
        return acknowledge(run, loaded, found, record, &acted, all.values).await;
    }

    // Read back, normalize, compare; one re-read before a mismatch.
    let mut reads_made = 0;
    let (raw, seen) = loop {
        let from_response = reads_made == 0
            && binding.evidence == WriteEvidence::Response
            && record
                .fields
                .iter()
                .filter(|field| record.intended.contains(&field.name))
                .all(|field| all.values.contains(&field.name));
        let raw = if from_response || binding.read_back.is_empty() {
            Some(all.values.clone())
        } else {
            match read_record(run, found, record, held, false).await {
                Ok(raw) => raw,
                Err(Stop::Fail(failure)) => {
                    // The record was written and could not be read: it is
                    // in doubt, whatever the read failed with.
                    let failure = after_send(failure, "the read-back");
                    fail_record(run, record, &failure, None).await;
                    return Err(Stop::Fail(failure));
                }
                Err(stop) => return Err(stop),
            }
        };
        let seen = match raw
            .as_ref()
            .map(|raw| compare::seen(loaded, &record.fields, &record.intended, raw))
            .transpose()
        {
            Ok(seen) => seen,
            Err(failure) => {
                fail_record(run, record, &failure, None).await;
                return Err(Stop::Fail(failure));
            }
        };
        if seen.as_ref() == Some(&record.intended) {
            break (raw, seen);
        }
        reads_made += 1;
        if reads_made > 1 {
            let differing = match &seen {
                Some(seen) => match compare::compare(&record.fields, &record.intended, seen) {
                    compare::Comparison::Differs(names) => names.join(", "),
                    compare::Comparison::Equal => String::new(),
                },
                None => "the record is absent".to_owned(),
            };
            let failure = codes::fail(
                FailureClass::Mismatch,
                codes::READ_BACK_DIFFERS,
                format!("The record read back differs from what was intended: {differing}."),
                "Compare the values in the evidence; the system changed what was written.",
            );
            let evidence = read_evidence(run, record, raw.as_ref(), seen.as_ref())
                .await
                .ok();
            fail_record(run, record, &failure, evidence).await;
            return Err(Stop::Fail(failure));
        }
    };

    // Record: evidence first, then `verified`. A record whose evidence
    // cannot be stored stays unverified.
    let evidence = read_evidence(run, record, raw.as_ref(), seen.as_ref()).await?;
    let seen = seen.unwrap_or_default();
    let after = store(run, BlobKind::Values, encode_values(&kept(run, &seen))?).await?;
    run.control
        .verified(
            &acted,
            ReadBack {
                digest: seen.digest(),
                after: Some(after),
                evidence,
            },
        )
        .await?;
    record.state = RecordState::Verified;
    let mut observed = record.known.clone();
    for (name, value) in all.values {
        observed.insert(name, value);
    }
    if let Some(raw) = raw {
        for (name, value) in raw {
            observed.insert(name, value);
        }
    }
    record.outputs = outputs_of(loaded, found, &observed, false)?;
    Ok(())
}

/// Ends a record whose only evidence is the system's acknowledgement: the
/// save was sent and answered, nothing is read back, and nothing is
/// compared. What the answer showed is kept as evidence, and the record
/// stays `acted`, never called verified (decision 0161).
async fn acknowledge(
    run: &mut Run<'_>,
    loaded: &Loaded,
    found: &OperationPlan,
    record: &mut Planned,
    acted: &ActedToken,
    answer: Values,
) -> Result<(), Stop> {
    let evidence = read_evidence(run, record, Some(&answer), None).await.ok();
    if let Err(failure) = run.control.acknowledged(acted, evidence).await {
        // The write was sent and answered, and nothing ends the record: it
        // stays in doubt, with its lock, for reconcile.
        let failure = after_send(failure, "keeping the system's acknowledgement");
        fail_record(run, record, &failure, None).await;
        return Err(Stop::Fail(failure));
    }
    record.state = RecordState::Acted;
    record.acknowledged = true;
    // Its outputs are what its steps gave up to and with the save, which
    // the ledger stored with its `acted` mark: the same a resume hands back.
    // A confirm step's values are evidence, not outputs.
    record.outputs = outputs_of(loaded, found, &record.known, false)?;
    Ok(())
}

/// A key the system assigned, as the ledger keeps it: its parts in the
/// canonical form.
fn key_text(key: &RecordKey) -> String {
    String::from_utf8_lossy(&key.canonical_bytes()).into_owned()
}
