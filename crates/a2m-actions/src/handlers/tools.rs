//! Using tools: finding them, calling one, and following its run.

use std::time::Duration;

use a2m_core::{
    EffectClass, Environment, Failure, FailureCode, Identity, OperationName, RunId, RunState,
    ToolRuns, Values,
};
use a2m_engine::encode_values;
use a2m_ledger::{BlobKind, Durability, NewBlob, NewRun, RetentionClass, RunRow, ToolKind};
use a2m_policy::PrincipalKind;
use a2m_sched::{
    Admitted, CancelOutcome, Enqueue, HealthState as SchedHealth, JobKind, JobRequirements,
    Priority, SessionLimits, Submission,
};
use a2m_vault::OwnerRef;
use serde_json::{Value as Json, json};

use super::handler;
use crate::codes;
use crate::dispatch::{Call, ExitClass, Next, Output};
use crate::health::HealthState;
use crate::kernel::Kernel;
use crate::person::{ApprovalAsk, SignInAsk};
use crate::runner::{CallerFacts, keep_run_call};

/// How long a call waits for its run to rest or end, when it does not say.
const DEFAULT_WAIT_SECONDS: i64 = 60;

handler!(
    /// `tool.search`: the published tools, by words or module.
    tool_search,
    |kernel, call| {
        let query = call.text("query").unwrap_or("").to_lowercase();
        let (plans, unreadable) = kernel.current_plans().await?;
        let mut tools = Vec::new();
        for (target, _, plan) in &plans {
            if call.text("target").is_some_and(|wanted| wanted != target.as_str()) {
                continue;
            }
            for tool in &plan.tools {
                if call
                    .text("module")
                    .is_some_and(|wanted| tool.module.as_deref() != Some(wanted))
                {
                    continue;
                }
                let hay = format!("{} {} {}", tool.name, tool.title, tool.description).to_lowercase();
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
                tools.push(json!({
                    "name": tool.name.as_str(),
                    "target": target.as_str(),
                    "version": plan.header.version.to_string(),
                    "kind": kind,
                    "effect": effect.map_or_else(|| "unknown".to_owned(), |effect| effect.to_string()),
                    "description": tool.description,
                }));
            }
        }
        let total = tools.len();
        let offset = usize::try_from(call.int("offset").unwrap_or(0)).unwrap_or(0);
        let limit = usize::try_from(call.int("limit").unwrap_or(50)).unwrap_or(50);
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
        let (effect, hazard) = found.effect();
        let schema = |text: &a2m_core::CanonicalJson| {
            serde_json::from_slice::<Json>(text.as_bytes()).unwrap_or(Json::Null)
        };
        Ok(Output::ok(json!({
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
        }))
        .then(Next::new(
            "tool.call",
            json!({"tool": found.tool.name.as_str(), "input": {}}),
            "to call it",
        )))
    }
);

fn sched_health(kernel: &Kernel) -> SchedHealth {
    let health = kernel.health();
    let cause = || {
        health
            .reasons
            .first()
            .map_or(codes::ENGINE_DEGRADED, |reason| reason.failure.code().clone())
    };
    match health.state {
        HealthState::Ok => SchedHealth::Ready,
        HealthState::Degraded => SchedHealth::Degraded { cause: cause() },
        HealthState::Stopped => SchedHealth::Stopped { cause: cause() },
    }
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
        session_limits: SessionLimits { reads: 4, writes: 1 },
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
                format!("The target `{}` has no environment named `{environment}`.", found.target),
                "Leave `environment` out to use the target's default.",
            ));
        }
        let store = kernel.store()?;
        let principal = &call.caller.principal;
        let identity = (principal.principal.kind == PrincipalKind::Person)
            .then_some(Identity::Person(principal.principal.id));
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
            input_hash: input.digest(),
            input: Some(stored),
            retention: RetentionClass::Long,
        };
        let facts = CallerFacts::of(&call.caller, call.decision);
        let kept = run.clone();
        store
            .write(Durability::Synced, move |tx| keep_run_call(tx, &kept, stored, &facts))
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
                    session_limits: SessionLimits { reads: 4, writes: 1 },
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
            .await?;
        let run = match admitted {
            Admitted::New { run, .. } | Admitted::Existing { run } => run,
        };
        let row = kernel.wait_for_rest(run, wait_of(&call)).await?;
        run_output(kernel, &call, row).await
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
        let row = self
            .store()?
            .read(move |tx| tx.ledger().run(run))
            .await?;
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

    /// Waits until the run rests, stops, or ends, or until `wait` has
    /// passed, and gives the run as it then stands. It waits on the runner's
    /// own notice that a run changed, never on a timer alone.
    pub(crate) async fn wait_for_rest(&self, run: RunId, wait: Duration) -> Result<RunRow, Failure> {
        let store = self.store()?;
        let mut changed = self.run_watch();
        let give_up = tokio::time::sleep(wait);
        tokio::pin!(give_up);
        loop {
            changed.borrow_and_update();
            let row = store.read(move |tx| tx.ledger().run(run)).await?;
            if !matches!(
                row.state,
                RunState::Queued | RunState::Validating | RunState::Running
            ) && !self.in_hand(run)
            {
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

/// The run as `tool.call`, `run.get`, and `run.resume` return it, with the
/// next step for the state it is in.
async fn run_output(kernel: &Kernel, call: &Call, row: RunRow) -> Result<Output, Failure> {
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
        out.insert(
            "result".to_owned(),
            serde_json::to_value(&result).unwrap_or(Json::Null),
        );
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
            // The run's person is told, on a surface the caller cannot
            // reach. The link may be given to an agent: following it is
            // never an approval by itself.
            if let Some(diff_hash) = row.diff_hash {
                let asked = kernel
                    .person()
                    .ask_approval(ApprovalAsk {
                        run: row.id,
                        workspace: row.workspace,
                        started_by: row.principal,
                        tool: row.tool_name.to_string(),
                        diff_hash,
                        summary: format!("`{}` would write to `{}`.", row.tool_name, row.target),
                    })
                    .await;
                if let Ok(link) = asked {
                    out.insert("link".to_owned(), json!(link.url));
                    out.insert("person".to_owned(), json!(link.text));
                }
            }
            (
                "The write waits for a person's approval. Nothing was written.".to_owned(),
                ExitClass::PersonNeeded,
                Some(Next::new("run.resume", again, "after the person approves")),
            )
        }
        RunState::NeedsLogin => {
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
                .await;
            if let Ok(link) = asked {
                out.insert("link".to_owned(), json!(link.url));
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
        run_output(kernel, &call, row).await
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
            RunState::NeedsApproval | RunState::NeedsLogin | RunState::NeedsInput | RunState::Stopped
        ) {
            return run_output(kernel, &call, row).await;
        }
        // The caller's facts are kept again, so that the guard's question
        // is asked about whoever resumes.
        let facts = CallerFacts::of(&call.caller, call.decision);
        let for_run = row.clone();
        kernel
            .store()?
            .write(Durability::Synced, move |tx| crate::runner::rekeep_caller(tx, &for_run, &facts))
            .await?;
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
        match queued {
            Ok(_) => {}
            // A job for the run is waiting or leased already, such as the
            // reconcile the scheduler queued after a lost lease.
            Err(failure) if failure.code() == &codes::SCHEMA_INVALID => {}
            Err(failure) => return Err(failure),
        }
        kernel.run_changed();
        let row = kernel.wait_for_work(run, row.epoch, wait_of(&call)).await?;
        run_output(kernel, &call, row).await
    }
);

impl Kernel {
    /// Waits, after a resume, until a new lease has worked on the run and
    /// it rests, stops, or ends again.
    async fn wait_for_work(
        &self,
        run: RunId,
        before: a2m_core::Epoch,
        wait: Duration,
    ) -> Result<RunRow, Failure> {
        let store = self.store()?;
        let mut changed = self.run_watch();
        let give_up = tokio::time::sleep(wait);
        tokio::pin!(give_up);
        loop {
            changed.borrow_and_update();
            let row = store.read(move |tx| tx.ledger().run(run)).await?;
            let worked = row.epoch != before;
            if worked
                && !matches!(
                    row.state,
                    RunState::Queued | RunState::Validating | RunState::Running
                )
                && !self.in_hand(run)
            {
                return Ok(row);
            }
            if row.state.is_final() {
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
        Ok(Output::ok(json!({"run_id": run.to_string(), "summary": said})).then(Next::new(
            "run.get",
            json!({"run_id": run.to_string()}),
            "to see what was completed before it stopped",
        )))
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
