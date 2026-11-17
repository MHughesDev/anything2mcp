//! Using tools: finding them, calling one, and following its run.

use std::time::Duration;

use a2m_core::{
    EffectClass, Environment, Failure, FailureCode, FieldHasher, Identity, OperationName, RunId,
    RunState, ToolRuns, Values,
};
use a2m_engine::encode_values;
use a2m_ledger::{
    BlobKind, CallerKey, Durability, NewBlob, NewRun, RetentionClass, RunQuery, RunRow, ToolKind,
};
use a2m_policy::{Context, ModuleName, PrincipalKind, Question, Resource, ToolRef};
use a2m_provider::{Access, EntryKind, RootName, Roots};
use a2m_sched::{
    Admitted, CancelOutcome, Enqueue, HealthState as SchedHealth, JobKind, JobRequirements,
    Priority, ResourceKey, Scheduler, SessionLimits, Submission,
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
                if call.text("kind").is_some_and(|wanted| wanted != kind) {
                    continue;
                }
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
async fn given_source(
    kernel: &Kernel,
    call: &Call,
    found: &FoundTool,
) -> Result<Option<GivenSource>, Failure> {
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
            let folder = call.project_folder()?.to_path_buf();
            // A source is read from the project's `sources` folder and from
            // nowhere else in the project: not its target files, not its
            // project file, not a hidden file.
            crate::project::source_path(path).map_err(|why| {
                codes::input(
                    codes::PATH_NOT_ALLOWED,
                    format!("The source file of this call is not read: {why}."),
                    format!(
                        "Put the file in the project's `{}` folder and give its path from the project folder, such as `{}/items.csv`.",
                        crate::project::SOURCES_FOLDER,
                        crate::project::SOURCES_FOLDER
                    ),
                )
            })?;
            // The size ceiling is the smaller of what a caller may hand in
            // here and what a source may be at all.
            let most = a2m_engine::EngineSettings::default().source_limits.bytes;
            let most = kernel
                .settings()
                .int("files.max_upload_bytes")
                .and_then(|bytes| u64::try_from(bytes).ok())
                .map_or(most, |allowed| allowed.min(most));
            let wanted = path.to_owned();
            // Looking at the disk and reading a file wait: off the async
            // threads. The checked path follows a link only while it stays
            // inside the project, and what stands at the name must be a
            // file: a link, a device, or a pipe is not read.
            let bytes = tokio::task::spawn_blocking(move || {
                let root = RootName::new("project")?;
                let mut roots = Roots::none();
                roots.add(root.clone(), &folder, Access::Read)?;
                let file = roots.folder(&root, Access::Read)?.join(&wanted)?;
                match file.entry_kind()? {
                    EntryKind::File => file.read(most),
                    EntryKind::Absent => Err(codes::input(
                        a2m_provider::codes::FILE_UNAVAILABLE,
                        "The source file is not there.",
                        "Check the path, from the project folder.",
                    )),
                    _ => Err(codes::input(
                        codes::PATH_NOT_ALLOWED,
                        "The source file of this call is not read: what stands at its name is not a file.",
                        "Give the path of a file; a link, a folder, or a device is not read.",
                    )),
                }
            })
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

fn requirements(run: &RunRow, kind: JobKind, exclusive: Vec<ResourceKey>) -> JobRequirements {
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
        exclusive,
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
        let (effect, hazard) = found.effect();
        // The decision about this call, and the health check before it,
        // were made for the tool as it stood then. A version published in
        // between may have made a read a write: the call is then not the
        // one that was decided, and it is refused.
        if call.decided != Some((effect, hazard)) {
            return Err(codes::busy(
                codes::TOOL_CHANGED,
                format!(
                    "`{name}` was published again while this call was being decided, with another effect."
                ),
                "Call the tool again.",
            ));
        }
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
        let given = given_source(kernel, &call, &found).await?;
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
        // A call with an idempotency key is the same call as the first
        // one made with it: its run is given back, unfinished or finished,
        // and no other is started (decision 0160). Two calls with one key
        // are one at a time from the look at the key to the binding of
        // the run, so the second finds what the first bound.
        let keyed = match call.idempotency_key.as_deref() {
            Some(text) => Some(Keyed {
                key: CallerKey::new(text)?,
                request: request_digest(&name, &environment, &input_hash),
            }),
            None => None,
        };
        let _one_at_a_time = match &keyed {
            Some(keyed) => Some(
                kernel
                    .inner
                    .key_locks
                    .of(keyed.lock_name(principal))
                    .lock_owned()
                    .await,
            ),
            None => None,
        };
        if let Some(keyed) = &keyed {
            let looked =
                looked_up(kernel, principal, keyed, (&name, &environment, &input_hash)).await?;
            match looked {
                Looked::Free | Looked::Held => {}
                Looked::Reused => {
                    return Err(codes::input(
                        codes::IDEMPOTENCY_KEY_REUSED,
                        "This idempotency key was given before with another call: another tool, environment, input, or source. Nothing was started.",
                        "Use a new key for a new call. To get the run of the call the key was first given with, make that call again with the key, or call `run.list` with `idempotency_key`.",
                    ));
                }
                Looked::Bound(run) => {
                    drop(_one_at_a_time);
                    let row = kernel.wait_for_rest(run, wait_of(&call)).await?;
                    return repeated(run_output(kernel, &call, row, Tell::Person).await?);
                }
            }
        }
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
        let to_hold = keyed.clone();
        store
            .write(Durability::Synced, move |tx| {
                keep_run_call(tx, &kept, stored, &facts)?;
                if let Some(held) = &to_hold {
                    // Held before the run is admitted: a call stopped in
                    // between leaves the key held, and the next call with
                    // it looks for the run before it starts another.
                    tx.ledger().hold_caller_key(
                        &kept.workspace,
                        &kept.principal,
                        &held.key,
                        &held.request,
                    )?;
                }
                match &source {
                    Some(source) => keep_run_source(tx, &kept, source),
                    None => Ok(()),
                }
            })
            .await?;
        let work_target = run.target.clone();
        let work_environment = environment.clone();
        let exclusive = kernel
            .held_alone(
                principal.workspace,
                &found.plan,
                &name,
                &environment,
                effect != EffectClass::Read,
            )
            .await?;
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
                    exclusive,
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
                // Nothing was admitted, so the key is let go: the same call
                // may be made again with it.
                if let Some(keyed) = &keyed {
                    let (workspace, person, key) = (
                        principal.workspace,
                        principal.principal.id,
                        keyed.key.clone(),
                    );
                    let _ = store
                        .write(Durability::Synced, move |tx| {
                            tx.ledger().release_caller_key(&workspace, &person, &key)
                        })
                        .await;
                }
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
        if let Some(keyed) = &keyed {
            let (workspace, person, key) = (
                principal.workspace,
                principal.principal.id,
                keyed.key.clone(),
            );
            store
                .write(Durability::Synced, move |tx| {
                    tx.ledger().bind_caller_key(&workspace, &person, &key, run)
                })
                .await?;
        }
        drop(_one_at_a_time);
        let row = kernel.wait_for_rest(run, wait_of(&call)).await?;
        run_output(kernel, &call, row, Tell::Person).await
    }
);

/// A call's idempotency key, and the digest of what the call asks for.
#[derive(Clone)]
struct Keyed {
    key: CallerKey,
    request: a2m_core::Digest,
}

impl Keyed {
    /// The name of the lock for this key of this caller, in memory only.
    fn lock_name(&self, principal: &crate::dispatch::Principal) -> String {
        let mut hasher = FieldHasher::new();
        hasher.field("purpose", b"anything2mcp caller key lock 1");
        hasher.field("workspace", principal.workspace.to_string().as_bytes());
        hasher.field("principal", principal.principal.id.to_string().as_bytes());
        hasher.field("key", self.key.as_str().as_bytes());
        hasher.finish().to_string()
    }
}

/// What a call asks for, as a key is held to it: the tool, the
/// environment, and the input with its source. The version of the tool is
/// not part of it: the same call made after a publish is still the same
/// call, and its key still gives back its run.
fn request_digest(
    name: &OperationName,
    environment: &Environment,
    input_hash: &a2m_core::Digest,
) -> a2m_core::Digest {
    let mut hasher = FieldHasher::new();
    hasher.field("purpose", b"anything2mcp keyed call 1");
    hasher.field("tool", name.as_str().as_bytes());
    hasher.field("environment", environment.as_str().as_bytes());
    hasher.field("input", input_hash.to_string().as_bytes());
    hasher.finish()
}

/// What the store holds for a call's key.
enum Looked {
    /// Nothing: the call is the first with this key.
    Free,
    /// Held by an earlier call with the same request that admitted no run.
    Held,
    /// Given before with another request.
    Reused,
    /// Bound to this run.
    Bound(RunId),
}

/// Looks a call's key up, and binds a held key to the run its earlier call
/// was admitted as when that call stopped before it could.
async fn looked_up(
    kernel: &Kernel,
    principal: &crate::dispatch::Principal,
    keyed: &Keyed,
    call: (&OperationName, &Environment, &a2m_core::Digest),
) -> Result<Looked, Failure> {
    let (workspace, person) = (principal.workspace, principal.principal.id);
    let (key, request) = (keyed.key.clone(), keyed.request);
    let (tool, environment, input_hash) = (call.0.clone(), call.1.clone(), *call.2);
    kernel
        .store()?
        .write(Durability::Synced, move |tx| {
            let mut ledger = tx.ledger();
            let Some(row) = ledger.caller_key(&workspace, &person, &key)? else {
                return Ok(Looked::Free);
            };
            if row.request != request {
                return Ok(Looked::Reused);
            }
            if let Some(run) = row.run {
                return Ok(Looked::Bound(run));
            }
            match ledger.run_called_since(
                &workspace,
                &person,
                (&tool, &environment, &input_hash),
                row.held_at,
            )? {
                Some(run) => {
                    ledger.bind_caller_key(&workspace, &person, &key, run)?;
                    Ok(Looked::Bound(run))
                }
                None => Ok(Looked::Held),
            }
        })
        .await
}

/// The output of a call that was made before with the same key: the run
/// it started, marked as given back.
fn repeated(mut output: Output) -> Result<Output, Failure> {
    if let Json::Object(map) = &mut output.result {
        map.insert("repeated".to_owned(), json!(true));
    }
    Ok(output.warn(
        "This call was made before with the same idempotency key: this is the run it started, and no other was started.",
    ))
}

/// The most runs `run.list` returns when the call names no limit.
const LIST_PAGE: u32 = 20;

handler!(
    /// `run.list`: the caller's own runs, newest first, with what a caller
    /// needs to decide what to do next with each.
    run_list,
    |kernel, call| {
        let principal = &call.caller.principal;
        // A run is its person's alone; a caller that is no person, and
        // acts for none, has no runs. No role lists another person's runs:
        // an auditor reads them through `trace.show` and the audit log.
        let Some(person) = principal.person() else {
            return Ok(Output::ok(json!({"runs": [], "complete": true})));
        };
        let invalid = |message: &str, fix: &str| codes::input(codes::SCHEMA_INVALID, message, fix);
        let states = match call.input.get("state") {
            Some(Json::Array(items)) => items
                .iter()
                .map(|item| {
                    item.as_str()
                        .and_then(|text| text.parse::<RunState>().ok())
                        .ok_or_else(|| {
                            invalid(
                                "A state to list by is not a run's state.",
                                "Use states such as `queued`, `running`, `needs_approval`, `stopped`, `completed`, or `failed`.",
                            )
                        })
                })
                .collect::<Result<Vec<_>, _>>()?,
            _ => Vec::new(),
        };
        let after = call
            .text("after")
            .map(|text| {
                text.parse::<RunId>().map_err(|_| {
                    invalid(
                        "`after` is not a run's identifier.",
                        "Pass the `after` the previous page gave.",
                    )
                })
            })
            .transpose()?;
        let limit = call
            .int("limit")
            .and_then(|limit| u32::try_from(limit).ok())
            .unwrap_or(LIST_PAGE)
            .clamp(1, 100);
        let query = RunQuery {
            workspace: principal.workspace,
            principal: person,
            states,
            tool: call.text("tool").map(OperationName::new).transpose()?,
            target: call
                .text("target")
                .map(a2m_core::TargetName::new)
                .transpose()?,
            since: call.int("since").map(a2m_core::Timestamp::from_unix_millis),
            caller_key: call
                .text("idempotency_key")
                .map(CallerKey::new)
                .transpose()?,
            after,
            // One more than is shown, to know whether the list goes on.
            limit: limit + 1,
        };
        let listed = kernel
            .store()?
            .read(move |tx| {
                let rows = tx.ledger().runs(&query)?;
                // Whether a record is in doubt is read from the ledger, and
                // is yes when it cannot be read.
                let doubts = rows
                    .iter()
                    .map(|row| {
                        tx.ledger()
                            .in_doubt(row.id)
                            .map_or(true, |found| !found.is_empty())
                    })
                    .collect::<Vec<bool>>();
                Ok((rows, doubts))
            })
            .await
            .map_err(|failure| {
                if failure.code() == &codes::NOT_FOUND {
                    no_such_run()
                } else {
                    failure
                }
            })?;
        let (mut rows, doubts) = listed;
        let complete = rows.len() <= limit as usize;
        rows.truncate(limit as usize);
        let runs: Vec<Json> = rows
            .iter()
            .zip(doubts)
            .map(|(row, in_doubt)| listed_run(kernel, row, in_doubt))
            .collect();
        let mut out = json!({"runs": runs, "complete": complete});
        let mut output = Output::ok(Json::Null);
        if !complete && let Some(last) = rows.last() {
            out["after"] = json!(last.id.to_string());
            let mut args = call.input.clone();
            args.insert("after".to_owned(), json!(last.id.to_string()));
            output = output.then(Next::new(
                "run.list",
                Json::Object(args),
                "for the next page",
            ));
        }
        output.result = out;
        Ok(output)
    }
);

/// One run as `run.list` shows it.
fn listed_run(kernel: &Kernel, row: &RunRow, in_doubt: bool) -> Json {
    let approved = row.state == RunState::NeedsApproval
        && row
            .diff_hash
            .is_some_and(|diff| kernel.inner.signals.approved(row.id, &diff));
    let waits = matches!(
        row.state,
        RunState::NeedsApproval | RunState::NeedsLogin | RunState::NeedsInput
    ) && !approved;
    let again = json!({"run_id": row.id.to_string()});
    let next = match row.state {
        RunState::Queued | RunState::Validating | RunState::Running => {
            Next::new("run.get", again, "in a few seconds")
        }
        RunState::NeedsApproval if approved => Next::new("run.resume", again, "now"),
        RunState::NeedsApproval => Next::new("run.resume", again, "after the person approves"),
        RunState::NeedsLogin => Next::new("run.resume", again, "after the person signs in"),
        RunState::NeedsInput => Next::new("run.resume", again, "after the person answers"),
        RunState::Stopped => Next::new(
            "run.resume",
            again,
            "to continue the run; every record in doubt is read first",
        ),
        RunState::Completed => Next::new("run.get", again, "to read its result"),
        RunState::Expired => Next::new(
            "tool.call",
            json!({"tool": row.tool_name.as_str()}),
            "with the same input, if the write is still wanted",
        ),
        _ => Next::new("trace.show", again, "to see what happened"),
    };
    let mut entry = json!({
        "run_id": row.id.to_string(),
        "tool": row.tool_name.as_str(),
        "target": row.target.as_str(),
        "status": row.state.as_str(),
        "created_at": row.created_at.as_unix_millis(),
        "in_doubt": in_doubt,
        "waits_for_person": waits,
        "next": {"action": next.action, "args": next.args, "when": next.when},
    });
    if let Some(started) = row.started_at {
        entry["started_at"] = json!(started.as_unix_millis());
    }
    if let Some(ended) = row.finished_at {
        entry["ended_at"] = json!(ended.as_unix_millis());
    }
    if let Some((class, code)) = &row.failure {
        entry["failure"] = json!({"class": class.to_string(), "code": code.to_string()});
    }
    entry
}

pub(crate) fn wait_of(call: &Call) -> Duration {
    let seconds = call.int("wait_seconds").unwrap_or(DEFAULT_WAIT_SECONDS);
    Duration::from_secs(u64::try_from(seconds).unwrap_or(0))
}

fn no_such_run() -> Failure {
    codes::input(
        codes::NOT_FOUND,
        "There is no such run.",
        "Call `run.list` to see your runs.",
    )
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
    /// The run, when it is the caller's own: in the caller's workspace, and
    /// started by the person the caller is or was proved to act for. A run
    /// is read, resumed, and cancelled by that person and by nobody else,
    /// whatever role they hold: a run acts under its person's session, and
    /// a session is its person's alone. Another person's run is answered
    /// as no run at all.
    async fn own_run(&self, call: &Call, run: RunId) -> Result<RunRow, Failure> {
        let row = self.run_in_workspace(call, run).await?;
        if call.caller.principal.person() == Some(row.principal) {
            Ok(row)
        } else {
            Err(no_such_run())
        }
    }

    /// The run, when it belongs to the caller's workspace, whoever started
    /// it. Only for `trace.show`: reading a run's evidence is what the
    /// auditor's role is for, and the dispatcher's decision was that the
    /// caller holds the role the action needs. It shows what happened and
    /// can neither resume nor change the run.
    async fn run_in_workspace(&self, call: &Call, run: RunId) -> Result<RunRow, Failure> {
        let row = self.store()?.read(move |tx| tx.ledger().run(run)).await?;
        if row.workspace == call.caller.principal.workspace {
            Ok(row)
        } else {
            Err(no_such_run())
        }
    }

    /// Asks the policy engine whether whoever resumes a run may use its
    /// tool, and records the answer (spec section 212, step 9). The run's
    /// own caller, kept when it was made, is asked about again at guard.
    async fn may_resume(&self, call: &Call, row: &RunRow) -> Result<(), Failure> {
        let policy = self.policy().cloned().ok_or_else(|| {
            codes::busy(
                codes::ENGINE_DEGRADED,
                "No policy set is loaded, so nothing can be decided.",
                "Call `doctor.run` for the fix.",
            )
        })?;
        let tool = self
            .plan_by_hash(&row.target, &row.target_version)
            .await
            .ok()
            .and_then(|plan| {
                let tool = plan.tool(&row.tool_name)?.clone();
                let hazard = match tool.runs {
                    ToolRuns::Operation(index) => plan.operation(index).map(|found| found.hazard),
                    ToolRuns::Pipeline(index) => plan.pipeline(index).map(|found| found.hazard),
                };
                Some((tool.module, hazard?))
            });
        let (module, hazard) = tool.unwrap_or((None, a2m_core::Hazard::None));
        let caller = &call.caller;
        let mut context = Context::plain(caller.principal.workspace, row.environment.clone());
        context.effect = row.effect;
        context.hazard = hazard;
        context.toolset = caller.toolset.clone();
        context.caller_is_agent = caller.is_agent;
        context.acting_for = caller.principal.acting_for.clone();
        let decision = policy
            .decide_alone(a2m_policy::Request {
                principal: caller.principal.principal.clone(),
                question: Question::UseTool,
                resource: Resource::Tool(ToolRef {
                    target: row.target.clone(),
                    module: ModuleName::new(module.as_deref().unwrap_or("main"))?,
                    tool: row.tool_name.clone(),
                }),
                context,
                run: Some(row.id),
                diff_hash: None,
            })
            .await;
        match decision.refusal {
            Some(refusal) => Err(refusal),
            None if decision.allowed() => Ok(()),
            None => Err(codes::policy(
                a2m_policy::NOT_GRANTED,
                "Policy did not allow this caller to use the run's tool.",
                "Ask an administrator for a grant on this tool.",
            )),
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
pub(crate) enum Tell {
    /// `tool.call` and `run.resume`.
    Person,
    /// `run.get`.
    Nobody,
}

/// The run as `tool.call`, `run.get`, and `run.resume` return it, with the
/// next step for the state it is in.
pub(crate) async fn run_output(
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
                out.insert(
                    "person".to_owned(),
                    json!("A person approved this write. It waits to be resumed."),
                );
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
        let row = kernel.own_run(&call, run_id(&call)?).await?;
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
        let row = kernel.own_run(&call, run).await?;
        if !matches!(
            row.state,
            RunState::NeedsApproval
                | RunState::NeedsLogin
                | RunState::NeedsInput
                | RunState::Stopped
        ) {
            return run_output(kernel, &call, row, Tell::Nobody).await;
        }
        // Whoever resumes must be allowed the run's tool now; a kill
        // switch that covers the work answers here as well. The facts of
        // the caller that started the run are left as they were kept: a
        // resume never changes whose run it is, or what its caller was.
        kernel.may_resume(&call, &row).await?;
        // What the run's tool holds alone, as when the run was made. A run
        // whose plan cannot be read is not queued: its job would be handed
        // out without what it must hold.
        let plan = kernel
            .plan_by_hash(&row.target, &row.target_version)
            .await?;
        let exclusive = kernel
            .held_alone(
                row.workspace,
                &plan,
                &row.tool_name,
                &row.environment,
                row.effect != EffectClass::Read,
            )
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
                    job: requirements(&row, JobKind::Execute, exclusive.clone()),
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
        kernel.own_run(&call, run).await?;
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
        let row = kernel.run_in_workspace(&call, run).await?;
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
            // The lines are checked against their own chain first: a trace
            // with a line missing, added, or moved is shown as broken, and
            // its signature is not passed on as if it stood.
            if let Ok(opened) = kernel.store()?.blobs().get(&trace).await
                && let Ok(lines) = a2m_telemetry::read_trace(opened.expose())
            {
                let check = a2m_telemetry::check_trace(&lines);
                map.insert("trace_check".to_owned(), json!(check.as_str()));
                if check != a2m_telemetry::TraceCheck::Broken
                    && let Some(a2m_telemetry::TraceLine::End(end)) = lines.last()
                {
                    map.insert("signature".to_owned(), json!(end.signature.to_string()));
                    map.insert("records".to_owned(), json!(end.records));
                }
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

handler!(
    /// `draft.try`: runs one drafted read in the engine's sandbox, as a
    /// `Try` job of its own run, and returns its outcome as `tool.call`
    /// would. The drafts are compiled as a draft; the plan is kept in
    /// memory for the run and never becomes a version, so no caller can
    /// reach it through `tool.call`. A drafted write is not tried: a trial
    /// that writes a declared test record needs the publish gates' test
    /// records, which are not built.
    draft_try,
    |kernel, call| {
        let target = a2m_core::TargetName::new(call.required("target")?)?;
        let name = OperationName::new(call.required("tool")?)?;
        let project_folder = call.project_folder()?.to_path_buf();
        let compiled = super::build::compile_in(
            kernel,
            project_folder,
            &target,
            a2m_files::CompileMode::Draft,
            a2m_files::DRAFT_VERSION,
        )
        .await?
        .map_err(Failure::from)?;
        let plan = std::sync::Arc::new(compiled.plan);
        let Some(tool) = plan.tool(&name).cloned() else {
            return Err(codes::input(
                codes::NOT_FOUND,
                format!("The drafts of `{target}` hold no tool named `{name}`."),
                "Call `draft.lint` to list the drafted tools.",
            ));
        };
        let found = FoundTool {
            target: target.clone(),
            plan_hash: compiled.encoded.hash,
            plan: std::sync::Arc::clone(&plan),
            tool,
        };
        let (effect, _) = found.effect();
        if effect != EffectClass::Read {
            return Err(codes::input(
                codes::ACTION_NOT_BUILT,
                format!("`{name}` is a drafted {effect}, and only a drafted read is tried today."),
                "Try the tool's reads; a write is proven by the publish gates on a declared test record.",
            ));
        }
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
        let environment = plan
            .default_environment()
            .map(|environment| environment.name.clone())
            .ok_or_else(|| {
                codes::input(
                    codes::NOT_FOUND,
                    "The drafted target declares no environment.",
                    "Declare an environment in the target's files.",
                )
            })?;
        kernel.keep_draft(&target, compiled.encoded.hash, std::sync::Arc::clone(&plan));
        let run = submit_trial(kernel, &call, &found, &input, environment).await?;
        let row = kernel.wait_for_rest(run, wait_of(&call)).await?;
        let output = run_output(kernel, &call, row, Tell::Person).await?;
        Ok(output.warn("This was a trial of a draft in the sandbox; nothing was published."))
    }
);

/// Keeps the call of a trial of one drafted read and submits it as a `Try`
/// job of a run of its own; the plan must be kept in memory already
/// (`Kernel::keep_draft`). Gives the run.
pub(crate) async fn submit_trial(
    kernel: &Kernel,
    call: &Call,
    found: &FoundTool,
    input: &Values,
    environment: Environment,
) -> Result<RunId, Failure> {
    let (name, target, effect) = (
        found.tool.name.clone(),
        found.target.clone(),
        found.effect().0,
    );
    let store = kernel.store()?;
    let principal = &call.caller.principal;
    let stored = store
        .blobs()
        .put(NewBlob {
            workspace: principal.workspace,
            owner: OwnerRef::of(&Identity::Person(principal.principal.id)),
            kind: BlobKind::Input,
            class: RetentionClass::Long,
            plain: encode_values(input)?,
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
        target: target.clone(),
        target_version: found.plan_hash,
        environment: environment.clone(),
        identity: (principal.principal.kind == PrincipalKind::Person)
            .then_some(Identity::Person(principal.principal.id)),
        effect,
        input_hash: {
            // A trial is another run than a call of the same input.
            let mut hasher = FieldHasher::new();
            hasher.field("purpose", b"anything2mcp draft try 1");
            hasher.field("input", input.digest().to_string().as_bytes());
            hasher.finish()
        },
        input: Some(stored),
        retention: RetentionClass::Long,
    };
    let facts = CallerFacts::of(&call.caller, call.decision);
    let kept = run.clone();
    store
        .write(Durability::Synced, move |tx| {
            keep_run_call(tx, &kept, stored, &facts)
        })
        .await?;
    let admitted = kernel
        .sched()?
        .submit(Submission {
            job: JobRequirements {
                kind: JobKind::Try,
                target: run.target.clone(),
                environment,
                modes: Vec::new(),
                os: None,
                acts_as: Some(run.principal),
                session: None,
                session_limits: SessionLimits {
                    reads: 4,
                    writes: 0,
                },
                exclusive: Vec::new(),
                max_step: Duration::from_secs(30),
                writes: false,
            },
            run,
            priority: Priority::Interactive,
            health: sched_health(kernel),
            queue_wait: None,
            budgets: Vec::new(),
        })
        .await?;
    let run = match admitted {
        Admitted::New { run, .. } | Admitted::Existing { run } => run,
    };
    Ok(run)
}

/// The plan a target's discovery or trial works on: its drafts, compiled
/// as a draft and kept in memory, when the call is made in a project that
/// holds the target; its current published version otherwise.
pub(crate) async fn plan_to_observe(
    kernel: &Kernel,
    call: &Call,
    target: &a2m_core::TargetName,
) -> Result<(a2m_core::Digest, std::sync::Arc<a2m_core::Plan>), Failure> {
    if let Ok(folder) = call.project_folder() {
        let folder = folder.to_path_buf();
        let named = target.clone();
        let present = super::build::off_thread(move || {
            let target_folder = super::build::target_folder(&folder, &named)?;
            Ok(target_folder.is_dir().then_some(folder))
        })
        .await?;
        if let Some(folder) = present {
            let compiled = super::build::compile_in(
                kernel,
                folder,
                target,
                a2m_files::CompileMode::Draft,
                a2m_files::DRAFT_VERSION,
            )
            .await?
            .map_err(Failure::from)?;
            let plan = std::sync::Arc::new(compiled.plan);
            kernel.keep_draft(target, compiled.encoded.hash, std::sync::Arc::clone(&plan));
            return Ok((compiled.encoded.hash, plan));
        }
    }
    let (plans, _) = kernel.current_plans().await?;
    plans
        .into_iter()
        .find(|(name, _, _)| name == target)
        .map(|(_, hash, plan)| (hash, plan))
        .ok_or_else(|| {
            codes::input(
                codes::NOT_FOUND,
                format!("No target named `{target}` is in the project or published."),
                "Add it with `target.add`, or call `target.list`.",
            )
        })
}

/// The name a discovery run's stored call is kept under.
pub(crate) const DISCOVER_TOOL: &str = "a2m_discover";

/// Keeps the call of a discovery of one surface and submits it as a
/// `Discover` job of a run of its own, in the engine's sandbox. Gives the
/// run. A discovery of the same surface of the same plan by the same
/// caller that has not ended is given back, not started again.
pub(crate) async fn submit_discovery(
    kernel: &Kernel,
    call: &Call,
    target: &a2m_core::TargetName,
    plan_hash: a2m_core::Digest,
    environment: &Environment,
    surface: &a2m_core::SurfaceName,
) -> Result<RunId, Failure> {
    let store = kernel.store()?;
    let principal = &call.caller.principal;
    let tool = OperationName::new(DISCOVER_TOOL)?;
    let input = Values::new()
        .with(
            crate::runner::DISCOVERY_SURFACE,
            a2m_core::Value::Text(surface.to_string()),
        )
        .with(
            crate::runner::DISCOVERY_FOCUS,
            a2m_core::Value::Record(Values::new()),
        );
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
        tool_name: tool,
        tool_kind: ToolKind::Operation,
        target: target.clone(),
        target_version: plan_hash,
        environment: environment.clone(),
        identity: (principal.principal.kind == PrincipalKind::Person)
            .then_some(Identity::Person(principal.principal.id)),
        effect: EffectClass::Read,
        input_hash: input.digest(),
        input: Some(stored),
        retention: RetentionClass::Long,
    };
    let facts = CallerFacts::of(&call.caller, call.decision);
    let kept = run.clone();
    store
        .write(Durability::Synced, move |tx| {
            keep_run_call(tx, &kept, stored, &facts)
        })
        .await?;
    let admitted = kernel
        .sched()?
        .submit(Submission {
            job: JobRequirements {
                kind: JobKind::Discover,
                target: run.target.clone(),
                environment: environment.clone(),
                modes: Vec::new(),
                os: None,
                acts_as: Some(run.principal),
                session: None,
                session_limits: SessionLimits {
                    reads: 4,
                    writes: 0,
                },
                exclusive: Vec::new(),
                max_step: Duration::from_secs(30),
                writes: false,
            },
            run,
            priority: Priority::Interactive,
            health: sched_health(kernel),
            queue_wait: None,
            budgets: Vec::new(),
        })
        .await?;
    Ok(match admitted {
        Admitted::New { run, .. } | Admitted::Existing { run } => run,
    })
}

handler!(
    /// `discover.run`: one `Discover` job of its own run for each surface
    /// of the target, or the one named, in the engine's sandbox, where a
    /// discoverer is given only a read-only port and no intent token
    /// exists. What the discoverers find is stored by the run's control as
    /// it is sent; the result counts it by kind.
    discover_run,
    |kernel, call| {
        let target = a2m_core::TargetName::new(call.required("target")?)?;
        let (plan_hash, plan) = plan_to_observe(kernel, &call, &target).await?;
        let surfaces: Vec<a2m_core::SurfaceName> = plan
            .surfaces
            .iter()
            .map(|surface| surface.name.clone())
            .filter(|name| {
                call.text("surface")
                    .is_none_or(|wanted| name.as_str() == wanted)
            })
            .collect();
        if surfaces.is_empty() {
            return Err(codes::input(
                codes::NOT_FOUND,
                format!("The target `{target}` declares no such surface."),
                "Name a surface the target's files declare, or leave `surface` out.",
            ));
        }
        let environment = plan
            .default_environment()
            .map(|environment| environment.name.clone())
            .ok_or_else(|| {
                codes::input(
                    codes::NOT_FOUND,
                    "The target declares no environment.",
                    "Declare an environment in the target's files.",
                )
            })?;
        let mut runs = Vec::new();
        for surface in &surfaces {
            let run =
                submit_discovery(kernel, &call, &target, plan_hash, &environment, surface).await?;
            runs.push((surface.clone(), run));
        }
        let mut shown = Vec::new();
        let mut all_ended = true;
        for (surface, run) in runs {
            let row = kernel.wait_for_rest(run, wait_of(&call)).await?;
            all_ended &= row.state.is_final();
            let mut entry = serde_json::Map::new();
            entry.insert("surface".to_owned(), json!(surface.as_str()));
            entry.insert("run_id".to_owned(), json!(run.to_string()));
            entry.insert("status".to_owned(), json!(row.state.as_str()));
            if let Some((class, code)) = row.failure.as_ref() {
                entry.insert(
                    "failure".to_owned(),
                    json!({"class": class.to_string(), "code": code.to_string()}),
                );
            }
            shown.push(Json::Object(entry));
        }
        let workspace = call.caller.principal.workspace;
        let listed = target.clone();
        let findings = kernel
            .store()?
            .read(move |tx| tx.findings().list(&workspace, &listed))
            .await?;
        let mut by_kind = std::collections::BTreeMap::<String, u64>::new();
        for finding in &findings {
            *by_kind.entry(finding.kind().to_string()).or_default() += 1;
        }
        let output = Output::ok(json!({
            "target": target.as_str(),
            "plan_hash": plan_hash.to_string(),
            "surfaces": shown,
            "findings": findings.len(),
            "by_kind": by_kind,
            "complete": all_ended,
        }))
        .then(Next::new(
            "finding.list",
            json!({"target": target.as_str()}),
            "to read what was found",
        ));
        Ok(output)
    }
);

handler!(
    /// `finding.list`: the stored findings about a target, in the order
    /// they were observed, paged.
    finding_list,
    |kernel, call| {
        let target = a2m_core::TargetName::new(call.required("target")?)?;
        let workspace = call.caller.principal.workspace;
        let listed = target.clone();
        let findings = kernel
            .store()?
            .read(move |tx| tx.findings().list(&workspace, &listed))
            .await?;
        let kind = call.text("kind");
        let chosen: Vec<&a2m_core::Finding> = findings
            .iter()
            .filter(|finding| kind.is_none_or(|kind| finding.kind().as_str() == kind))
            .collect();
        let offset = usize::try_from(call.int("offset").unwrap_or(0)).unwrap_or(0);
        let limit = usize::try_from(call.int("limit").unwrap_or(50)).unwrap_or(50);
        let page: Vec<Json> = chosen
            .iter()
            .skip(offset)
            .take(limit)
            .map(|finding| {
                json!({
                    "id": finding.id().to_string(),
                    "kind": finding.kind().as_str(),
                    "level": finding.level().as_str(),
                    "source": finding.source().as_str(),
                })
            })
            .collect();
        let complete = offset + page.len() >= chosen.len();
        Ok(Output::ok(json!({
            "target": target.as_str(),
            "findings": page,
            "total": chosen.len(),
            "complete": complete,
        })))
    }
);

handler!(
    /// `finding.describe`: one stored finding in full.
    finding_describe,
    |kernel, call| {
        let target = a2m_core::TargetName::new(call.required("target")?)?;
        let id: a2m_core::Digest = call.required("finding")?.parse().map_err(|_| {
            codes::input(
                codes::SCHEMA_INVALID,
                "A finding's identifier is the hexadecimal digest `finding.list` gives.",
                "Pass an identifier from `finding.list`.",
            )
        })?;
        let workspace = call.caller.principal.workspace;
        let named = target.clone();
        let found = kernel
            .store()?
            .read(move |tx| tx.findings().get(&workspace, &named, &id))
            .await?
            .ok_or_else(|| {
                codes::input(
                    codes::NOT_FOUND,
                    format!("The target `{target}` has no finding with that identifier."),
                    "Call `finding.list` for the identifiers.",
                )
            })?;
        let whole = serde_json::to_value(&found).map_err(|_| {
            codes::input(
                codes::SCHEMA_INVALID,
                "The finding could not be written as JSON.",
                "Report this; it is a fault in the engine.",
            )
        })?;
        Ok(Output::ok(
            json!({"target": target.as_str(), "finding": whole}),
        ))
    }
);

handler!(
    /// `unknown.list`: the open questions about a target, as the unknowns
    /// ledger holds them.
    unknown_list,
    |kernel, call| {
        let target = a2m_core::TargetName::new(call.required("target")?)?;
        let workspace = call.caller.principal.workspace;
        let named = target.clone();
        let rows = kernel
            .store()?
            .read(move |tx| tx.findings().unknowns(&workspace, &named))
            .await?;
        let listed: Vec<Json> = rows
            .iter()
            .map(|row| {
                json!({
                    "id": row.id.to_string(),
                    "question": row.question,
                    "scope": row.scope,
                    "why_it_matters": row.why_it_matters,
                    "settled_by_what": row.settled_by_what,
                    "status": row.status.to_string(),
                    "opened_at": row.opened_at.as_unix_millis(),
                })
            })
            .collect();
        Ok(Output::ok(
            json!({"target": target.as_str(), "unknowns": listed}),
        ))
    }
);
