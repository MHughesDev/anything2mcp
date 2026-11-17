//! Using tools: finding them, calling one, and following its run.

use std::time::Duration;

use a2m_core::{
    EffectClass, Environment, Failure, FailureCode, FieldHasher, Identity, OperationName, RunId,
    RunState, ToolRuns, Values,
};
use a2m_engine::encode_values;
use a2m_ledger::{BlobKind, Durability, NewBlob, NewRun, RetentionClass, RunRow, ToolKind};
use a2m_policy::{Context, ModuleName, PrincipalKind, Question, Resource, ToolRef};
use a2m_provider::{Access, RootName, Roots};
use a2m_sched::{
    Admitted, CancelOutcome, Enqueue, HealthState as SchedHealth, JobKind, JobRequirements,
    Priority, Scheduler, SessionLimits, Submission,
};
use a2m_vault::OwnerRef;
use serde_json::{Value as Json, json};

use super::handler;
use crate::codes;
use crate::dispatch::{Call, ExitClass, Next, Output};
use crate::health::HealthState;
use crate::kernel::Kernel;
use crate::person::{ApprovalAsk, SignInAsk};
use crate::runner::{CallerFacts, KeptSource, SourceKind, keep_run_call, keep_run_source};
use crate::signals::KeptLink;
use crate::tools::FoundTool;

/// How many tools `tool.search` lists when the call names no limit.
const SHORT_PAGE: i64 = 50;
/// The same, when each tool is listed in full.
const FULL_PAGE: i64 = 20;

/// How long a call waits for its run to rest or end, when it does not say.
const DEFAULT_WAIT_SECONDS: i64 = 60;

handler!(
    /// `tool.search`: the published tools, by words or module.
    tool_search,
    |kernel, call| {
        let query = call.text("query").unwrap_or("").to_lowercase();
        let full = call.flag("full").unwrap_or(false);
        let (plans, unreadable) = kernel.current_plans().await?;
        let mut tools = Vec::new();
        for (target, plan_hash, plan) in &plans {
            if call
                .text("target")
                .is_some_and(|wanted| wanted != target.as_str())
            {
                continue;
            }
            for tool in &plan.tools {
                if call
                    .text("module")
                    .is_some_and(|wanted| tool.module.as_deref() != Some(wanted))
                {
                    continue;
                }
                let hay =
                    format!("{} {} {}", tool.name, tool.title, tool.description).to_lowercase();
                if !query.split_whitespace().all(|word| hay.contains(word)) {
                    continue;
                }
                let (effect, kind) = match tool.runs {
                    ToolRuns::Operation(index) => (
                        plan.operation(index).map(|operation| operation.effect),
                        "operation",
                    ),
                    ToolRuns::Pipeline(index) => (
                        plan.pipeline(index).map(|pipeline| pipeline.effect),
                        "pipeline",
                    ),
                };
                let mut listed = json!({
                    "name": tool.name.as_str(),
                    "target": target.as_str(),
                    "version": plan.header.version.to_string(),
                    "kind": kind,
                    "effect": effect.map_or_else(|| "unknown".to_owned(), |effect| effect.to_string()),
                    "description": tool.description,
                });
                // In full, each tool is what `tool.describe` gives for it,
                // so that a tool list costs one decided step and not one
                // for each tool.
                if full && let Json::Object(map) = &mut listed {
                    let found = FoundTool {
                        target: target.clone(),
                        plan_hash: *plan_hash,
                        plan: std::sync::Arc::clone(plan),
                        tool: tool.clone(),
                    };
                    if let Json::Object(described) = described(&found) {
                        map.extend(described);
                    }
                }
                tools.push(listed);
            }
        }
        let total = tools.len();
        let offset = usize::try_from(call.int("offset").unwrap_or(0)).unwrap_or(0);
        // A tool in full is some thousands of bytes, so a page of them is
        // shorter by default, to stay under the result size cap.
        let page_size = if full { FULL_PAGE } else { SHORT_PAGE };
        let limit = usize::try_from(call.int("limit").unwrap_or(page_size)).unwrap_or(50);
        let page: Vec<Json> = tools.into_iter().skip(offset).take(limit).collect();
        let complete = unreadable.is_empty() && offset + page.len() >= total;
        let mut output = Output::ok(json!({"tools": page, "total": total, "complete": complete}));
        for target in unreadable {
            output = output.warn(format!(
                "The plan of the target `{target}` cannot be read, so its tools are not listed. Publish it again from its sources."
            ));
        }
        Ok(output.then(Next::new(
            "tool.describe",
            json!({"tool": "<name>"}),
            "to read one tool's schema",
        )))
    }
);

handler!(
    /// `tool.describe`: one published tool in full.
    tool_describe,
    |kernel, call| {
        let name = OperationName::new(call.required("tool")?)?;
        let found = kernel.find_tool(&name).await?;
        Ok(Output::ok(described(&found)).then(Next::new(
            "tool.call",
            json!({"tool": found.tool.name.as_str(), "input": {}}),
            "to call it",
        )))
    }
);

/// The most bytes of rows a call may carry inline. A larger input is a
/// file.
const INLINE_ROWS_BYTES: usize = 256 * 1024;

/// The reader inline rows are read by: they are handed on as a JSON
/// document.
const ROWS_READER: &str = "json";

/// The source a call hands to a tool: its bytes, read and not yet stored.
struct GivenSource {
    kind: SourceKind,
    name: String,
    bytes: Vec<u8>,
}

/// Takes the source of a call. A file is named by its path inside the
/// project folder and read here, through a checked path, once; the engine
/// is handed bytes and never a path (spec section 210). Rows given inline
/// are the bytes of a JSON document.
///
/// # Errors
///
/// `input`/`schema_invalid` when the tool reads a source and the call gives
/// none, gives both forms, or gives one to a tool that reads none, and for
/// rows given to a source that another reader reads;
/// `input`/`path_not_allowed` and `input`/`file_unavailable` for the file.
async fn given_source(call: &Call, found: &FoundTool) -> Result<Option<GivenSource>, Failure> {
    let reads = match found.tool.runs {
        ToolRuns::Pipeline(index) => found
            .plan
            .pipeline(index)
            .and_then(|pipeline| pipeline.source.as_ref()),
        ToolRuns::Operation(_) => None,
    };
    let tool = &found.tool.name;
    let invalid = |message: String, fix: &str| codes::input(codes::SCHEMA_INVALID, message, fix);
    match (
        reads,
        call.text("source_file"),
        call.input.get("source_rows"),
    ) {
        (None, None, None) => Ok(None),
        (None, _, _) => Err(invalid(
            format!("`{tool}` reads no source, and the call gives one."),
            "Leave `source_file` and `source_rows` out.",
        )),
        (Some(_), None, None) => Err(invalid(
            format!("`{tool}` reads a source, and the call gives none."),
            "Give `source_file`, the file's path inside the project folder, or `source_rows` for a small input.",
        )),
        (Some(_), Some(_), Some(_)) => Err(invalid(
            format!("The call gives `{tool}` both a source file and rows."),
            "Give `source_file` or `source_rows`, not both.",
        )),
        (Some(_), Some(path), None) => {
            let folder = call.project.as_ref().ok_or_else(|| {
                codes::input(
                    codes::PATH_NOT_ALLOWED,
                    "A source file is read from a project folder, and the call names none.",
                    "Run the command inside the project folder, or pass `--project` with the folder's full path.",
                )
            })?;
            let root = RootName::new("project")?;
            let mut roots = Roots::none();
            roots.add(root.clone(), folder, Access::Read)?;
            let file = roots.folder(&root, Access::Read)?.join(path)?;
            let most = a2m_engine::EngineSettings::default().source_limits.bytes;
            // Reading a file waits on the disk: off the async threads.
            let bytes = tokio::task::spawn_blocking(move || file.read(most))
                .await
                .map_err(|_| {
                    codes::input(
                        a2m_provider::codes::FILE_UNAVAILABLE,
                        "The source file could not be read.",
                        "Try again.",
                    )
                })??;
            Ok(Some(GivenSource {
                kind: SourceKind::File,
                name: codes::shown(path),
                bytes,
            }))
        }
        (Some(source), None, Some(rows)) => {
            if source.reader != ROWS_READER {
                return Err(invalid(
                    format!(
                        "The source of `{tool}` is read by the `{}` reader, which takes a file; rows given inline are read as a JSON document.",
                        source.reader
                    ),
                    "Give `source_file`, the file's path inside the project folder.",
                ));
            }
            let bytes = serde_json::to_vec(rows).map_err(|_| {
                invalid(
                    "The rows could not be written as a JSON document.".to_owned(),
                    "Give `source_rows` as a list of records.",
                )
            })?;
            if bytes.len() > INLINE_ROWS_BYTES {
                return Err(invalid(
                    format!(
                        "The rows are {} bytes, and at most {INLINE_ROWS_BYTES} may be given inline.",
                        bytes.len()
                    ),
                    "Write the rows to a file in the project and give `source_file`.",
                ));
            }
            Ok(Some(GivenSource {
                kind: SourceKind::Rows,
                name: String::new(),
                bytes,
            }))
        }
    }
}

/// One published tool in full, as `tool.describe` returns it and as
/// `tool.search` lists it when asked for everything.
fn described(found: &FoundTool) -> Json {
    let (effect, hazard) = found.effect();
    // A schema is a map. A plan that carries none for a tool says only
    // that the value is an object.
    let schema =
        |text: &a2m_core::CanonicalJson| match serde_json::from_slice::<Json>(text.as_bytes()) {
            Ok(Json::Object(map)) => Json::Object(map),
            _ => json!({"type": "object"}),
        };
    json!({
        "name": found.tool.name.as_str(),
        "target": found.target.as_str(),
        "version": found.plan.header.version.to_string(),
        "plan_hash": found.plan_hash.to_string(),
        "kind": found.kind(),
        "effect": effect.to_string(),
        "hazard": hazard.to_string(),
        "title": found.tool.title,
        "description": found.tool.description,
        "input_schema": schema(&found.tool.input_schema),
        "output_schema": schema(&found.tool.output_schema),
        "annotations": {
            "read_only": found.tool.annotations.read_only,
            "destructive": found.tool.annotations.destructive,
            "idempotent": found.tool.annotations.idempotent,
            "open_world": found.tool.annotations.open_world,
        },
    })
}

fn sched_health(kernel: &Kernel) -> SchedHealth {
    let health = kernel.health();
    let cause = || {
        health
            .reasons
            .first()
            .map_or(codes::ENGINE_DEGRADED, |reason| {
                reason.failure.code().clone()
            })
    };
    match health.state {
        HealthState::Ok => SchedHealth::Ready,
        HealthState::Degraded => SchedHealth::Degraded { cause: cause() },
        HealthState::Stopped => SchedHealth::Stopped { cause: cause() },
    }
}

/// What a caller is told when the scheduler will not take work because a
/// kill switch covers it.
///
/// A kill switch is the policy engine's (spec section 212): it is checked
/// first on every decision about work on a system, and its refusal is
/// `policy`, `kill_switch_on`. The scheduler's hold is the same switch seen
/// from the queue, so its `busy`, `on_hold` is not passed on. The question
/// is put to the policy engine, which records its refusal, and that refusal
/// is the answer: a caller sees one code for a kill switch, wherever it
/// was met.
async fn held(kernel: &Kernel, call: &Call, work: &HeldWork<'_>, refused: Failure) -> Failure {
    if refused.code() != &a2m_sched::codes::ON_HOLD {
        return refused;
    }
    let switched = || {
        codes::policy(
            a2m_policy::KILL_SWITCH_ON,
            "A kill switch covers this work, so it was not started.",
            "Ask an administrator to release it with `killswitch.set`, then call again.",
        )
    };
    let (Some(policy), Ok(module)) = (
        kernel.policy().cloned(),
        ModuleName::new(work.module.unwrap_or("main")),
    ) else {
        return switched();
    };
    let caller = &call.caller;
    let mut context = Context::plain(caller.principal.workspace, work.environment.clone());
    context.effect = work.effect;
    context.toolset = caller.toolset.clone();
    context.caller_is_agent = caller.is_agent;
    context.acting_for = caller.principal.acting_for.clone();
    let decision = policy
        .decide_alone(a2m_policy::Request {
            principal: caller.principal.principal.clone(),
            question: Question::UseTool,
            resource: Resource::Tool(ToolRef {
                target: work.target.clone(),
                module,
                tool: work.tool.clone(),
            }),
            context,
            run: work.run,
            diff_hash: None,
        })
        .await;
    match decision.refusal {
        Some(refusal) if refusal.code() == &a2m_policy::KILL_SWITCH_ON => refusal,
        _ => switched(),
    }
}

/// The work a kill switch was met on.
struct HeldWork<'a> {
    target: &'a a2m_core::TargetName,
    module: Option<&'a str>,
    tool: &'a OperationName,
    environment: &'a Environment,
    effect: EffectClass,
    run: Option<RunId>,
}

fn requirements(run: &RunRow, kind: JobKind) -> JobRequirements {
    JobRequirements {
        kind,
        target: run.target.clone(),
        environment: run.environment.clone(),
        modes: Vec::new(),
        os: None,
        acts_as: Some(run.principal),
        session: None,
        session_limits: SessionLimits {
            reads: 4,
            writes: 1,
        },
        exclusive: Vec::new(),
        max_step: Duration::from_secs(30),
        writes: run.effect != EffectClass::Read,
    }
}

handler!(
    /// `tool.call`: submits a run of one published tool and returns its
    /// outcome, or the state it rests in.
    tool_call,
    |kernel, call| {
        let name = OperationName::new(call.required("tool")?)?;
        let found = kernel.find_tool(&name).await?;
        let (effect, _) = found.effect();
        let input: Values = match call.input.get("input") {
            Some(given) => serde_json::from_value(given.clone()).map_err(|_| {
                codes::input(
                    codes::SCHEMA_INVALID,
                    "The tool's input holds a value that cannot be taken.",
                    "Use text, whole numbers, true or false, lists, and maps; write an exact number as text.",
                )
            })?,
            None => Values::new(),
        };
        let environment = match call.text("environment") {
            Some(given) => Environment::new(given)?,
            None => found
                .plan
                .default_environment()
                .map(|environment| environment.name.clone())
                .ok_or_else(|| {
                    codes::input(
                        codes::NOT_FOUND,
                        "The tool's target declares no environment.",
                        "Declare an environment in the target's files and publish again.",
                    )
                })?,
        };
        if found.plan.environment_named(&environment).is_none() {
            return Err(codes::input(
                codes::NOT_FOUND,
                format!(
                    "The target `{}` has no environment named `{environment}`.",
                    found.target
                ),
                "Leave `environment` out to use the target's default.",
            ));
        }
        let given = given_source(&call, &found).await?;
        let store = kernel.store()?;
        let principal = &call.caller.principal;
        let identity = (principal.principal.kind == PrincipalKind::Person)
            .then_some(Identity::Person(principal.principal.id));
        // The source is stored before the run is admitted, under a name
        // that is a keyed digest of its bytes. That name is part of what
        // makes a run the same run, so the same call with another file is
        // another run, and a resumed run reads the bytes it was given.
        let source = match given {
            Some(given) => {
                let size = given.bytes.len() as u64;
                let blob = store
                    .blobs()
                    .put(NewBlob {
                        workspace: principal.workspace,
                        owner: OwnerRef::of(&Identity::Person(principal.principal.id)),
                        kind: BlobKind::Input,
                        class: RetentionClass::Long,
                        plain: given.bytes,
                    })
                    .await?;
                Some(KeptSource {
                    blob,
                    kind: given.kind,
                    name: given.name,
                    size,
                })
            }
            None => None,
        };
        let input_hash = match &source {
            None => input.digest(),
            Some(source) => {
                let mut hasher = FieldHasher::new();
                hasher.field("purpose", b"anything2mcp call with source 1");
                hasher.field("input", input.digest().to_string().as_bytes());
                hasher.field("source", source.blob.to_string().as_bytes());
                hasher.finish()
            }
        };
        let stored = store
            .blobs()
            .put(NewBlob {
                workspace: principal.workspace,
                owner: OwnerRef::of(&Identity::Person(principal.principal.id)),
                kind: BlobKind::Input,
                class: RetentionClass::Long,
                plain: encode_values(&input)?,
            })
            .await?;
        let run = NewRun {
            workspace: principal.workspace,
            principal: principal.principal.id,
            client_label: codes::shown(&call.caller.client),
            tool_name: name.clone(),
            tool_kind: match found.tool.runs {
                ToolRuns::Operation(_) => ToolKind::Operation,
                ToolRuns::Pipeline(_) => ToolKind::Pipeline,
            },
            target: found.target.clone(),
            target_version: found.plan_hash,
            environment: environment.clone(),
            identity,
            effect,
            input_hash,
            input: Some(stored),
            retention: RetentionClass::Long,
        };
        let facts = CallerFacts::of(&call.caller, call.decision);
        let kept = run.clone();
        store
            .write(Durability::Synced, move |tx| {
                keep_run_call(tx, &kept, stored, &facts)?;
                match &source {
                    Some(source) => keep_run_source(tx, &kept, source),
                    None => Ok(()),
                }
            })
            .await?;
        let work_target = run.target.clone();
        let work_environment = environment.clone();
        let admitted = kernel
            .sched()?
            .submit(Submission {
                job: JobRequirements {
                    kind: JobKind::Execute,
                    target: run.target.clone(),
                    environment,
                    modes: Vec::new(),
                    os: None,
                    acts_as: Some(run.principal),
                    session: None,
                    session_limits: SessionLimits {
                        reads: 4,
                        writes: 1,
                    },
                    exclusive: Vec::new(),
                    max_step: Duration::from_secs(30),
                    writes: effect != EffectClass::Read,
                },
                run,
                priority: Priority::Interactive,
                health: sched_health(kernel),
                queue_wait: None,
                budgets: Vec::new(),
            })
            .await;
        let admitted = match admitted {
            Ok(admitted) => admitted,
            Err(refused) => {
                let work = HeldWork {
                    target: &work_target,
                    module: found.tool.module.as_deref(),
                    tool: &name,
                    environment: &work_environment,
                    effect,
                    run: None,
                };
                return Err(held(kernel, &call, &work, refused).await);
            }
        };
        let run = match admitted {
            Admitted::New { run, .. } | Admitted::Existing { run } => run,
        };
        let row = kernel.wait_for_rest(run, wait_of(&call)).await?;
        run_output(kernel, &call, row, Tell::Person).await
    }
);

fn wait_of(call: &Call) -> Duration {
    let seconds = call.int("wait_seconds").unwrap_or(DEFAULT_WAIT_SECONDS);
    Duration::from_secs(u64::try_from(seconds).unwrap_or(0))
}

fn run_id(call: &Call) -> Result<RunId, Failure> {
    call.required("run_id")?.parse::<RunId>().map_err(|_| {
        codes::input(
            codes::SCHEMA_INVALID,
            "That is not a run's identifier.",
            "Pass the `run_id` an earlier result gave.",
        )
    })
}

impl Kernel {
    /// The run, when it belongs to the caller's workspace.
    async fn run_row(&self, call: &Call, run: RunId) -> Result<RunRow, Failure> {
        let row = self.store()?.read(move |tx| tx.ledger().run(run)).await?;
        if row.workspace == call.caller.principal.workspace {
            Ok(row)
        } else {
            Err(codes::input(
                codes::NOT_FOUND,
                "There is no such run.",
                "Call `run.list` to see your runs.",
            ))
        }
    }

    /// The run, and whether the scheduler holds a job for it that is
    /// waiting or leased, read in one transaction. A job a runner has
    /// claimed is leased from the transaction of its claim to the one that
    /// records how it ended, so nothing the runner does between the two is
    /// taken for rest.
    async fn run_and_work(&self, run: RunId) -> Result<(RunRow, bool), Failure> {
        self.store()?
            .read(move |tx| {
                let row = tx.ledger().run(run)?;
                let working = Scheduler::unfinished_job(tx, run)?.is_some();
                Ok((row, working))
            })
            .await
    }

    /// Waits until the run has ended, or rests or has stopped with no job
    /// waiting or in hand for it, or until `wait` has passed, and gives the
    /// run as it then stands. It waits on the notice that a run changed,
    /// never on a timer alone.
    pub(crate) async fn wait_for_rest(
        &self,
        run: RunId,
        wait: Duration,
    ) -> Result<RunRow, Failure> {
        let mut changed = self.run_watch();
        let give_up = tokio::time::sleep(wait);
        tokio::pin!(give_up);
        loop {
            // The notice is marked seen before the look, so that a change
            // between the look and the wait is not missed.
            changed.borrow_and_update();
            let (row, working) = self.run_and_work(run).await?;
            let active = matches!(
                row.state,
                RunState::Queued | RunState::Validating | RunState::Running
            );
            if row.state.is_final() || !(active || working) {
                return Ok(row);
            }
            tokio::select! {
                alive = changed.changed() => {
                    if alive.is_err() {
                        return Ok(row);
                    }
                }
                () = &mut give_up => return Ok(row),
            }
        }
    }
}

/// Whether an action may tell the run's person that the run waits for
/// them. The actions that start or continue a run may; reading a run asks
/// nobody anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tell {
    /// `tool.call` and `run.resume`.
    Person,
    /// `run.get`.
    Nobody,
}

/// The run as `tool.call`, `run.get`, and `run.resume` return it, with the
/// next step for the state it is in.
async fn run_output(
    kernel: &Kernel,
    call: &Call,
    row: RunRow,
    tell: Tell,
) -> Result<Output, Failure> {
    let store = kernel.store()?;
    let run = row.id;
    let events = store
        .read(move |tx| tx.ledger().events(run, 0, 10_000))
        .await?;
    // The sentence and the next step of the failure that ended or rested
    // the run are in the event that changed its state.
    let said = events.iter().rev().find_map(|event| {
        let payload: Json = serde_json::from_str(&event.payload).ok()?;
        let message = payload.get("message")?.as_str()?.to_owned();
        let next_step = payload.get("next_step")?.as_str()?.to_owned();
        Some((message, next_step))
    });
    let mode = events.iter().rev().find_map(|event| {
        let payload: Json = serde_json::from_str(&event.payload).ok()?;
        payload
            .get("provider")
            .and(payload.get("mode"))
            .and_then(Json::as_str)
            .map(str::to_owned)
    });
    let failure = row.failure.as_ref().map(|(class, code)| {
        let (message, next_step) = said.clone().unwrap_or_else(|| {
            (
                format!("The run ended as `{}`.", row.state),
                "Call `trace.show` with the run's identifier to see what happened.".to_owned(),
            )
        });
        Failure::new(*class, code.clone(), message, next_step)
    });

    let mut out = serde_json::Map::new();
    out.insert("run_id".to_owned(), json!(row.id.to_string()));
    out.insert("tool".to_owned(), json!(row.tool_name.as_str()));
    out.insert("status".to_owned(), json!(row.state.as_str()));
    out.insert("events".to_owned(), json!(events.len()));
    if let Some(mode) = mode {
        out.insert("mode".to_owned(), json!(mode));
    }
    if let Some(result) = kernel.stored_values(row.result).await? {
        // A caller is shown the values themselves: an exact number as its
        // exact decimal text, not as the engine keeps it.
        out.insert("result".to_owned(), crate::render::plain_values(&result));
    }
    if let Some(diff) = row.diff_hash {
        out.insert(
            "diff".to_owned(),
            json!({
                "hash": diff.to_string(),
                "detail": row.diff.map(|blob| format!("blob:{blob}")),
            }),
        );
    }
    if let Some(trace) = row.trace {
        out.insert("trace".to_owned(), json!(format!("blob:{trace}")));
    }
    if let Some(failure) = &failure {
        out.insert(
            "failure".to_owned(),
            json!({
                "class": failure.class().to_string(),
                "code": failure.code().to_string(),
                "message": failure.message(),
                "fix": failure.next_step(),
            }),
        );
    }

    let again = json!({"run_id": row.id.to_string()});
    let (summary, exit, next): (String, ExitClass, Option<Next>) = match row.state {
        RunState::Completed => ("The run completed.".to_owned(), ExitClass::Succeeded, None),
        RunState::Queued | RunState::Validating | RunState::Running => (
            format!("The run is `{}`.", row.state),
            ExitClass::Succeeded,
            Some(Next::new("run.get", again, "in a few seconds")),
        ),
        RunState::NeedsApproval => {
            let signals = &kernel.inner.signals;
            if row
                .diff_hash
                .is_some_and(|diff| signals.approved(row.id, &diff))
            {
                // A person approved this diff at the desk and the run was
                // not resumed yet. Nobody is asked again; the next step is
                // the caller's own.
                out.insert("approved".to_owned(), json!(true));
                (
                    "A person approved the write. Nothing was written yet: resume the run to carry it out."
                        .to_owned(),
                    ExitClass::Succeeded,
                    Some(Next::new("run.resume", again, "now")),
                )
            } else {
                // The run's person is told, on a surface the caller cannot
                // reach, by the call that rested the run and by no other.
                // The link may be given to an agent: following it is never
                // an approval by itself.
                // A person is told once of one diff of one run: a call that
                // finds the run still resting gives the link it was given.
                let kept = signals.link(row.id, row.state, row.diff_hash.as_ref());
                let link = match (tell, row.diff_hash, kept) {
                    (_, _, Some(kept)) => Some(kept),
                    (Tell::Person, Some(diff_hash), None) => {
                        let asked = kernel
                            .person()
                            .ask_approval(ApprovalAsk {
                                run: row.id,
                                workspace: row.workspace,
                                started_by: row.principal,
                                tool: row.tool_name.to_string(),
                                diff_hash,
                                summary: format!(
                                    "`{}` would write to `{}`.",
                                    row.tool_name, row.target
                                ),
                            })
                            .await
                            .ok();
                        if let Some(link) = &asked {
                            signals.keep_link(
                                row.id,
                                KeptLink {
                                    state: row.state,
                                    diff: Some(diff_hash),
                                    link: link.clone(),
                                },
                            );
                        }
                        asked
                    }
                    _ => None,
                };
                if let Some(link) = link {
                    if let Some(url) = link.url {
                        out.insert("link".to_owned(), json!(url));
                    }
                    out.insert("person".to_owned(), json!(link.text));
                }
                (
                    "The write waits for a person's approval. Nothing was written.".to_owned(),
                    ExitClass::PersonNeeded,
                    Some(Next::new("run.resume", again, "after the person approves")),
                )
            }
        }
        RunState::NeedsLogin => {
            let signals = &kernel.inner.signals;
            let link = match tell {
                Tell::Person => {
                    let asked = kernel
                        .person()
                        .ask_sign_in(SignInAsk {
                            session: None,
                            workspace: row.workspace,
                            target: row.target.clone(),
                            environment: row.environment.clone(),
                            identity: Identity::Person(row.principal),
                            reason: a2m_auth::PersonReason::SignIn,
                        })
                        .await
                        .ok();
                    if let Some(link) = &asked {
                        signals.keep_link(
                            row.id,
                            KeptLink {
                                state: row.state,
                                diff: None,
                                link: link.clone(),
                            },
                        );
                    }
                    asked
                }
                Tell::Nobody => signals.link(row.id, row.state, None),
            };
            if let Some(link) = link {
                if let Some(url) = link.url {
                    out.insert("link".to_owned(), json!(url));
                }
                out.insert("person".to_owned(), json!(link.text));
            }
            (
                "The run waits for a person to sign in.".to_owned(),
                ExitClass::PersonNeeded,
                Some(Next::new("run.resume", again, "after the person signs in")),
            )
        }
        RunState::NeedsInput => (
            "The run waits for an answer from a person.".to_owned(),
            ExitClass::PersonNeeded,
            Some(Next::new("run.resume", again, "after the person answers")),
        ),
        RunState::Stopped => (
            "The run stopped and can be resumed. Resuming reconciles every doubtful record first."
                .to_owned(),
            ExitClass::Failed,
            Some(Next::new("run.resume", again, "to continue the run")),
        ),
        // A person declined the write, or the approval window ended. The
        // run is final: the same call again is a new run with a new diff.
        RunState::Expired => {
            // What the run's person did, in the words the desk recorded.
            if let Some(failure) = &failure {
                out.insert("person".to_owned(), json!(failure.message()));
            }
            (
                "The write was not approved. Nothing was written.".to_owned(),
                ExitClass::Failed,
                Some(Next::new(
                    "tool.call",
                    json!({"tool": row.tool_name.as_str()}),
                    "with the same input, if the write is still wanted",
                )),
            )
        }
        other => (
            format!("The run ended as `{other}`."),
            ExitClass::Failed,
            Some(Next::new("trace.show", again, "to see what happened")),
        ),
    };
    out.insert("summary".to_owned(), json!(summary));
    let _ = call;
    let mut output = Output::ok(Json::Object(out)).exit(exit);
    output.next = next;
    if exit == ExitClass::Failed {
        let failure = failure.unwrap_or_else(|| {
            Failure::new(
                a2m_core::FailureClass::Rejected,
                FailureCode::fixed("run_not_completed"),
                format!("The run ended as `{}`.", row.state),
                "Call `trace.show` with the run's identifier to see what happened.",
            )
        });
        output = output.failed(failure);
    }
    Ok(output)
}

handler!(
    /// `run.get`: where one run stands.
    run_get,
    |kernel, call| {
        let row = kernel.run_row(&call, run_id(&call)?).await?;
        run_output(kernel, &call, row, Tell::Nobody).await
    }
);

handler!(
    /// `run.resume`: queues the job that continues a run that rests or
    /// stopped, and returns where the run then stands. The engine
    /// reconciles before it writes; an approval is checked, and used, by
    /// the engine through the policy engine, never here.
    run_resume,
    |kernel, call| {
        let run = run_id(&call)?;
        let row = kernel.run_row(&call, run).await?;
        if !matches!(
            row.state,
            RunState::NeedsApproval
                | RunState::NeedsLogin
                | RunState::NeedsInput
                | RunState::Stopped
        ) {
            return run_output(kernel, &call, row, Tell::Nobody).await;
        }
        // The caller's facts are kept again, so that the guard's question
        // is asked about whoever resumes.
        let facts = CallerFacts::of(&call.caller, call.decision);
        let for_run = row.clone();
        kernel
            .store()?
            .write(Durability::Synced, move |tx| {
                crate::runner::rekeep_caller(tx, &for_run, &facts)
            })
            .await?;
        let give_up = tokio::time::Instant::now() + wait_of(&call);
        let mut row = row;
        // Twice at most: a job for the run may be waiting or leased already,
        // such as the reconcile the scheduler queued after a lost lease. A
        // reconcile settles what was in doubt and leaves the run stopped;
        // going on from there is this call's own job, queued once the
        // other has ended.
        for last in [false, true] {
            let queued = kernel
                .sched()?
                .enqueue(Enqueue {
                    run,
                    job: requirements(&row, JobKind::Execute),
                    priority: Priority::Interactive,
                    health: sched_health(kernel),
                    queue_wait: None,
                })
                .await;
            let own = match queued {
                Ok(_) => true,
                Err(failure) if failure.code() == &codes::SCHEMA_INVALID => false,
                Err(failure) => {
                    let module = kernel
                        .plan_by_hash(&row.target, &row.target_version)
                        .await
                        .ok()
                        .and_then(|plan| plan.tool(&row.tool_name)?.module.clone());
                    let work = HeldWork {
                        target: &row.target,
                        module: module.as_deref(),
                        tool: &row.tool_name,
                        environment: &row.environment,
                        effect: row.effect,
                        run: Some(run),
                    };
                    return Err(held(kernel, &call, &work, failure).await);
                }
            };
            kernel.run_changed();
            let left = give_up.saturating_duration_since(tokio::time::Instant::now());
            // The job, this call's own or the one that was there first, is
            // unfinished in the scheduler's table from before this wait
            // begins until its work has ended, so the wait cannot return in
            // between.
            row = kernel.wait_for_rest(run, left).await?;
            if own || last || row.state != RunState::Stopped || left.is_zero() {
                break;
            }
        }
        run_output(kernel, &call, row, Tell::Person).await
    }
);

handler!(
    /// `run.cancel`: stops a running or waiting run.
    run_cancel,
    |kernel, call| {
        let run = run_id(&call)?;
        kernel.run_row(&call, run).await?;
        let outcome = kernel.sched()?.cancel(run).await?;
        kernel.run_changed();
        let said = match outcome {
            CancelOutcome::Cancelled => "The run is cancelled. No runner held it.".to_owned(),
            CancelOutcome::Requested => "A runner holds the run. It finishes the commit point in hand and then cancels the run.".to_owned(),
            CancelOutcome::AlreadyFinal(state) => format!("The run had already ended as `{state}`."),
        };
        Ok(
            Output::ok(json!({"run_id": run.to_string(), "summary": said})).then(Next::new(
                "run.get",
                json!({"run_id": run.to_string()}),
                "to see what was completed before it stopped",
            )),
        )
    }
);

handler!(
    /// `trace.show`: one run's events, in order, and its trace reference.
    trace_show,
    |kernel, call| {
        let run = run_id(&call)?;
        let row = kernel.run_row(&call, run).await?;
        let events = kernel
            .store()?
            .read(move |tx| tx.ledger().events(run, 0, 10_000))
            .await?;
        let shown: Vec<Json> = events
            .iter()
            .map(|event| {
                json!({
                    "seq": event.seq,
                    "lease": event.epoch.get(),
                    "kind": event.kind.as_str(),
                    "detail": serde_json::from_str::<Json>(&event.payload).unwrap_or(Json::Null),
                    "evidence": event.evidence.map(|blob| format!("blob:{blob}")),
                })
            })
            .collect();
        let mut out = json!({"run_id": run.to_string(), "events": shown});
        if let (Some(trace), Json::Object(map)) = (row.trace, &mut out) {
            map.insert("trace".to_owned(), json!(format!("blob:{trace}")));
            // The trace signature is the last line of the stored trace. A
            // trace that cannot be opened or read is shown by its reference
            // alone.
            if let Ok(opened) = kernel.store()?.blobs().get(&trace).await
                && let Ok(lines) = a2m_telemetry::read_trace(opened.expose())
                && let Some(a2m_telemetry::TraceLine::End(end)) = lines.last()
            {
                map.insert("signature".to_owned(), json!(end.signature.to_string()));
                map.insert("records".to_owned(), json!(end.records));
            }
        }
        Ok(Output::ok(out))
    }
);

handler!(
    /// `plan.show`: a published plan as text.
    plan_show,
    |kernel, call| {
        let target = a2m_core::TargetName::new(call.required("target")?)?;
        let (plans, _) = kernel.current_plans().await?;
        let Some((_, hash, plan)) = plans.into_iter().find(|(name, _, _)| name == &target) else {
            return Err(codes::input(
                codes::NOT_FOUND,
                format!("The target `{target}` has no published version."),
                "Publish it with `version.publish`, or lint its drafts with `draft.lint`.",
            ));
        };
        Ok(Output::ok(json!({
            "target": target.as_str(),
            "version": plan.header.version.to_string(),
            "plan_hash": hash.to_string(),
            "text": plan.show()?,
        })))
    }
);
