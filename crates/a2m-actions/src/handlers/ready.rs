//! `draft.ready` (decisions 0401 and 0443): the steps between a read that
//! was tried and a publish a person carries out, as one action.
//!
//! For each drafted read a trial in this project read a value through, the
//! action records its offline test and fixture (`fixture.record`), promotes
//! its first binding on that fixture's own trial (`binding.promote`), then
//! runs the publish gates (`test.run`) and, when they pass, asks a person to
//! publish (`publish.request`). Each step is the existing action, dispatched
//! for the same caller, so each is decided by the policy engine and checked
//! by its own handler: the action adds no power. A read whose step fails is
//! left as it was and reported with that step's failure; the others go on.
//! A write, a pipeline, and a read no trial has read through are left alone,
//! each with its reason and the step that would move it on.
//!
//! A read is tried when the newest finished scout of the target in this
//! project proved it, or when the caller has a completed trial of it, made
//! from this project since that scout finished (or at all, with no scout).
//! The input is the one the scout's report carries, or that trial's own.

use a2m_core::{
    BindingStatus, EffectClass, Failure, FailureClass, FailureCode, OperationName, RunId, RunState,
    TargetName, Timestamp, ToolRuns,
};
use a2m_files::{CompileMode, DRAFT_VERSION};
use a2m_ledger::RunQuery;
use serde_json::{Map, Value as Json, json};

use super::build::{compile_in, off_thread, target_folder};
use super::handler;
use crate::codes;
use crate::dispatch::{Call, ExitClass, Next, Output, Request, Response};
use crate::kernel::Kernel;

/// How many of a read's newest completed runs are looked at for the
/// caller's own trial of it.
const TRIALS_LOOKED_AT: u32 = 20;

/// One drafted tool, as the drafts compile.
struct Drafted {
    tool: OperationName,
    /// `None` for a pipeline.
    effect: Option<EffectClass>,
    /// The mode of the binding that would act live: the first verified,
    /// else the first not retired.
    mode: Option<String>,
    /// Whether that binding is verified already.
    verified: bool,
}

/// Where the input a read is made ready with came from.
enum Source {
    /// The call the scout's report carries for it.
    Report(Json),
    /// A completed trial's own stored input.
    Trial(RunId, Json),
}

impl Source {
    fn input(&self) -> &Json {
        match self {
            Self::Report(input) | Self::Trial(_, input) => input,
        }
    }

    fn shown(&self) -> Json {
        match self {
            Self::Report(_) => json!({"from": "scout_report"}),
            Self::Trial(run, _) => json!({"from": "trial", "run_id": run.to_string()}),
        }
    }
}

/// What is done with one drafted tool.
enum Plan {
    /// Made ready with the input of this source, or ready before: verified,
    /// with its test and fixture (`true`). A read ready before may have no
    /// source.
    Make(Drafted, Option<Source>, bool),
    /// Left alone: the reason, why, and the step that moves it on.
    Leave(Json, (FailureCode, String, String)),
}

/// A step dispatched for the caller: the same request a face would make,
/// with the same project folder.
async fn step(kernel: &Kernel, call: &Call, action: &str, input: Json) -> Response {
    kernel
        .dispatch(Request {
            action: action.to_owned(),
            input,
            caller: call.caller.clone(),
            idempotency_key: None,
            project: call.project.clone(),
        })
        .await
}

/// A step's failure, as the result shows it.
fn step_failure(response: &Response) -> Option<Json> {
    response
        .error
        .as_ref()
        .map(crate::dispatch::ErrorBody::to_json)
}

/// The failure a response carries, as a failure again.
fn as_failure(response: &Response) -> Option<Failure> {
    response.error.as_ref().map(|error| {
        Failure::new(
            error.class,
            FailureCode::new(&error.code).unwrap_or(FailureCode::SCHEMA_INVALID),
            error.message.clone(),
            error.fix.clone(),
        )
    })
}

/// The caller's newest completed trial of `tool`, made from this project
/// since `since`, with its stored input: `None` when there is none whose
/// input is still kept.
async fn caller_trial(
    kernel: &Kernel,
    call: &Call,
    target: &TargetName,
    tool: &OperationName,
    since: Option<Timestamp>,
) -> Result<Option<(RunId, Json)>, Failure> {
    let principal = &call.caller.principal;
    let Some(person) = principal.person() else {
        return Ok(None);
    };
    let query = RunQuery {
        workspace: principal.workspace,
        principal: person,
        states: vec![RunState::Completed],
        tool: Some(tool.clone()),
        target: Some(target.clone()),
        since,
        caller_key: None,
        after: None,
        limit: TRIALS_LOOKED_AT,
    };
    let project = call.project.clone();
    let found = kernel
        .store()?
        .read(move |tx| {
            let rows = tx.ledger().runs(&query)?;
            let mut found = Vec::new();
            for row in rows {
                // A trial carries the binding it ran; a call of a published
                // tool carries none, and is no trial of a draft.
                if let Some(stored) = crate::runner::run_call(tx, &row)?
                    && stored.caller.binding.is_some()
                    && stored.caller.project.is_some()
                    && stored.caller.project == project
                {
                    found.push((row.id, stored.input));
                }
            }
            Ok(found)
        })
        .await?;
    for (run, blob) in found {
        // A trial's stored input is kept for its retention class; one whose
        // input is gone is passed over.
        if let Ok(Some(values)) = kernel.stored_values(Some(blob)).await
            && let Ok(input) = serde_json::to_value(&values)
        {
            return Ok(Some((run, input)));
        }
    }
    Ok(None)
}

/// The drafted tools of the compiled drafts, in their order.
fn drafted(compiled: &a2m_core::CompiledTarget) -> Vec<Drafted> {
    compiled
        .tools
        .iter()
        .map(|tool| {
            let operation = match tool.runs {
                ToolRuns::Operation(index) => compiled.operation(index),
                ToolRuns::Pipeline(_) => None,
            };
            let acting = operation.and_then(|operation| {
                a2m_engine::default_binding(operation)
                    .and_then(|place| operation.bindings.get(place))
            });
            Drafted {
                tool: tool.name.clone(),
                effect: operation.map(|operation| operation.effect),
                mode: acting.map(|binding| binding.mode.as_str().to_owned()),
                verified: acting.is_some_and(|binding| binding.status == BindingStatus::Verified),
            }
        })
        .collect()
}

/// The fixture's and the test's name `fixture.record` gives a tool.
fn fixture_name(target: &TargetName, tool: &OperationName) -> String {
    tool.as_str()
        .strip_prefix(&format!("{target}_"))
        .unwrap_or(tool.as_str())
        .to_owned()
}

/// Whether the test and the fixture `fixture.record` would write for
/// `name` are both there already.
async fn recorded_before(call: &Call, target: &TargetName, name: &str) -> Result<bool, Failure> {
    let (folder, target, name) = (
        call.project_folder()?.to_path_buf(),
        target.clone(),
        name.to_owned(),
    );
    off_thread(move || {
        let at = target_folder(&folder, &target)?;
        Ok(at.join(&format!("fixtures/{name}"))?.exists()
            && at.join(&format!("tests/{name}.yaml"))?.exists())
    })
    .await
}

/// What is done with one drafted tool: made ready, or left alone with its
/// reason.
async fn plan(
    kernel: &Kernel,
    call: &Call,
    target: &TargetName,
    scout: Option<&super::scout::ScoutTried>,
    drafted: Drafted,
) -> Result<Plan, Failure> {
    let tool = drafted.tool.as_str().to_owned();
    let leave = |reason: &str, why: String, next: Json, code: FailureCode, fix: String| {
        Plan::Leave(
            json!({"tool": tool, "reason": reason, "why": why, "next": next}),
            (code, why, fix),
        )
    };
    let try_next =
        json!({"action": "draft.try", "input": {"target": target.as_str(), "tool": tool}});
    match drafted.effect {
        None => {
            return Ok(leave(
                "pipeline",
                format!("`{tool}` is a pipeline: `draft.ready` makes reads ready alone."),
                json!({"action": "test.run", "input": {"target": target.as_str()}}),
                codes::NOT_MADE_READY_NOT_A_READ,
                format!(
                    "Write the offline test of `{tool}` with `draft.write`, then run `test.run`; or leave it out of `tools`."
                ),
            ));
        }
        Some(effect) if effect != EffectClass::Read => {
            return Ok(leave(
                "write",
                format!(
                    "`{tool}` writes: its offline test and fixture are written by hand, and its binding is promoted by a person."
                ),
                json!({"action": "test.run", "input": {"target": target.as_str()}}),
                codes::NOT_MADE_READY_NOT_A_READ,
                format!(
                    "Write the offline test and fixture of `{tool}` with `draft.write` and run `test.run`; a person then promotes its binding with `binding.promote`. Or leave it out of `tools`."
                ),
            ));
        }
        Some(_) => {}
    }
    // A read already verified, with its test and fixture, is ready before.
    let name = fixture_name(target, &drafted.tool);
    let done = drafted.verified && recorded_before(call, target, &name).await?;
    let entry = scout.and_then(|scout| {
        scout
            .entries
            .iter()
            .find(|entry| entry["tool"] == tool.as_str())
    });
    let proven = entry.is_some_and(|entry| entry["state"] == "proven");
    // The report carries the input of a proven read, unless it was taken
    // from a list read's answer; that one is in the scout's own trial.
    if proven
        && let Some(entry) = entry
        && entry.get("input_from").is_none()
    {
        let input = entry["call"]["input"]
            .get("input")
            .cloned()
            .unwrap_or_else(|| json!({}));
        return Ok(Plan::Make(drafted, Some(Source::Report(input)), done));
    }
    let since = if proven {
        None
    } else {
        scout.map(|scout| scout.finished_at)
    };
    if let Some((run, input)) = caller_trial(kernel, call, target, &drafted.tool, since).await? {
        return Ok(Plan::Make(drafted, Some(Source::Trial(run, input)), done));
    }
    if done {
        return Ok(Plan::Make(drafted, None, true));
    }
    let state = entry
        .and_then(|entry| entry["state"].as_str())
        .unwrap_or("");
    Ok(match state {
        "waiting" => leave(
            "waits_for_sign_in",
            format!("`{tool}` waits for a sign-in: no trial of it has read a value yet."),
            entry
                .and_then(|entry| entry.get("next"))
                .cloned()
                .unwrap_or(try_next),
            codes::NOT_MADE_READY_UNTRIED,
            "Ask the person to sign in with `session.login`, try the read with `draft.try`, then call `draft.ready` again.".to_owned(),
        ),
        "failed" => leave(
            "trial_failed",
            format!(
                "The scout's trial of `{tool}` failed{}, and no trial of it has completed since.",
                entry
                    .and_then(|entry| entry["failure"]["code"].as_str())
                    .map(|code| format!(" with `{code}`"))
                    .unwrap_or_default()
            ),
            try_next,
            codes::NOT_MADE_READY_UNTRIED,
            format!("Try `{tool}` with `draft.try` until a trial completes, then call `draft.ready` again."),
        ),
        _ => leave(
            "not_tried",
            format!("No trial in this project has read a value through `{tool}`."),
            try_next,
            codes::NOT_MADE_READY_UNTRIED,
            format!("Try `{tool}` with `draft.try` and an input it answers, such as an example of its key, then call `draft.ready` again."),
        ),
    })
}

/// The outcome of making one read ready: its entry under `ready`, or under
/// `failed`.
async fn make_ready(
    kernel: &Kernel,
    call: &Call,
    target: &TargetName,
    drafted: &Drafted,
    (source, done): (Option<&Source>, bool),
) -> Result<Json, Json> {
    let tool = drafted.tool.as_str();
    let mut entry = json!({"tool": tool, "mode": drafted.mode});
    if let Some(source) = source {
        entry["input"] = source.shown();
    }
    let source = match source {
        Some(source) if !done => source,
        _ => {
            entry["fixture"] = json!("recorded_before");
            entry["binding"] = json!("verified_before");
            return Ok(entry);
        }
    };
    let failed = |step: &str, response: &Response| json!({"tool": tool, "step": step, "error": step_failure(response)});
    let name = fixture_name(target, &drafted.tool);
    let mut evidence: Option<String> = match source {
        Source::Trial(run, _) => Some(run.to_string()),
        Source::Report(_) => None,
    };
    if recorded_before(call, target, &name).await.unwrap_or(false) {
        entry["fixture"] = json!("recorded_before");
    } else {
        let mut input = json!({"target": target.as_str(), "tool": tool, "input": source.input()});
        if let Some(mode) = &drafted.mode {
            input["mode"] = json!(mode);
        }
        let recorded = step(kernel, call, "fixture.record", input).await;
        if !recorded.ok {
            return Err(failed("fixture.record", &recorded));
        }
        let result = recorded.result.clone().unwrap_or(Json::Null);
        // The fixture's own trial is the newest evidence of the binding as
        // it is now, made from this project by this caller.
        if let Some(run) = result["trial"]["run_id"].as_str() {
            evidence = Some(run.to_owned());
        }
        entry["fixture"] = json!("recorded");
        entry["recorded"] = json!({
            "fixture": result["fixture"],
            "test": result["test"],
            "test_state": result["test_state"],
            "trial": result["trial"],
        });
    }
    let Some(mode) = &drafted.mode else {
        return Ok(entry);
    };
    let mut input = json!({"target": target.as_str(), "tool": tool, "mode": mode});
    if let Some(run) = &evidence {
        input["run_id"] = json!(run);
    }
    let promoted = step(kernel, call, "binding.promote", input).await;
    if !promoted.ok {
        return Err(failed("binding.promote", &promoted));
    }
    let result = promoted.result.unwrap_or(Json::Null);
    entry["binding"] = if result["promoted"] == true {
        json!("promoted")
    } else {
        json!("verified_before")
    };
    if let Some(by) = result["evidence"]["tried"]["by"].as_str() {
        entry["evidence"] = json!(by);
    }
    Ok(entry)
}

/// The sentence the result ends with: what was done, and what is left.
fn summary(ready: &[Json], left: &[Json], failed: &[Json], publish: &Json) -> String {
    let names = |list: &[Json]| {
        list.iter()
            .filter_map(|entry| entry["tool"].as_str())
            .map(|tool| format!("`{tool}`"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut text = Vec::new();
    // An answer that waits for a person says first that it succeeded
    // (decision 0458).
    if publish["state"] == "needs_person" {
        text.push(
            "Succeeded: every step before the publish is done, and the publish waits for a person."
                .to_owned(),
        );
    }
    let holds = || {
        publish["tools"]
            .as_array()
            .map(|tools| {
                tools
                    .iter()
                    .filter_map(Json::as_str)
                    .map(|tool| format!("`{tool}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default()
    };
    if ready.is_empty() {
        text.push("No read was made ready.".to_owned());
    } else {
        text.push(format!("Ready: {}.", names(ready)));
    }
    if !left.is_empty() {
        text.push(format!("Left alone: {}.", names(left)));
    }
    if !failed.is_empty() {
        text.push(format!("Failed: {}.", names(failed)));
    }
    text.push(match publish["state"].as_str() {
        Some("needs_person") => format!(
            "The version asked for holds {}. Give the person the link under `publish`, and stop until they have carried it out. Nothing else is left to do before then.",
            holds()
        ),
        Some("published") => format!(
            "Version {} is published, holding {}: the person carried out the publish.",
            publish["version"].as_str().unwrap_or("?"),
            holds()
        ),
        _ => "No publish was asked for: the publish gates did not pass.".to_owned(),
    });
    text.join(" ")
}

handler!(
    /// `draft.ready`: every drafted read a trial in this project read a
    /// value through gets its offline test and fixture recorded and its
    /// first binding promoted, then the publish gates run and, when they
    /// pass, a person is asked to publish (decisions 0401 and 0443). Each
    /// step is the existing action, dispatched for the same caller.
    draft_ready,
    |kernel, call| {
        let target = TargetName::new(call.required("target")?)?;
        let project_folder = call.project_folder()?.to_path_buf();
        let compiled = compile_in(
            kernel,
            project_folder,
            &target,
            CompileMode::Draft,
            DRAFT_VERSION,
        )
        .await?
        .map_err(Failure::from)?;
        let mut tools = drafted(&compiled.compiled_target);
        let named = super::build::chosen_tools(&call);
        // The tools the version asked for holds: those named, or every
        // drafted tool (decision 0457).
        let published: Vec<String> = named.clone().unwrap_or_else(|| {
            tools
                .iter()
                .map(|tool| tool.tool.as_str().to_owned())
                .collect()
        });
        if let Some(named) = &named {
            if let Some(missing) = named
                .iter()
                .find(|name| !tools.iter().any(|tool| tool.tool.as_str() == name.as_str()))
            {
                return Err(codes::input(
                    codes::NOT_FOUND,
                    format!(
                        "The drafts of `{target}` hold no tool named `{}`.",
                        codes::shown(missing)
                    ),
                    "Call `draft.lint` to list the drafted tools.",
                ));
            }
            tools.retain(|tool| named.iter().any(|name| name == tool.tool.as_str()));
        }
        let scout = super::scout::scout_tried(kernel, &call, &target).await?;

        // Every tool is looked at before any step is taken, so that a call
        // that names a tool which cannot be made ready changes nothing.
        let mut plans = Vec::with_capacity(tools.len());
        for tool in tools {
            plans.push(plan(kernel, &call, &target, scout.as_ref(), tool).await?);
        }
        let left: Vec<Json> = plans
            .iter()
            .filter_map(|plan| match plan {
                Plan::Leave(entry, _) => Some(entry.clone()),
                Plan::Make(..) => None,
            })
            .collect();
        if named.is_some()
            && let Some((code, why, fix)) = plans.iter().find_map(|plan| match plan {
                Plan::Leave(_, refusal) => Some(refusal.clone()),
                Plan::Make(..) => None,
            })
        {
            let next = left.first().and_then(|entry| {
                Some(Next::new(
                    entry["next"]["action"].as_str()?,
                    entry["next"]["input"].clone(),
                    "to move on the tool left alone, then call `draft.ready` again",
                ))
            });
            let publish = json!({"state": "not_asked"});
            let mut output = Output::ok(json!({
                "target": target.as_str(),
                "ready": [],
                "left_alone": left,
                "failed": [],
                "publish": publish,
                "text": format!("Nothing was done: {why}"),
            }))
            .failed(Failure::new(FailureClass::Input, code, why, fix));
            if let Some(next) = next {
                output = output.then(next);
            }
            return Ok(output);
        }

        let (mut ready, mut failed) = (Vec::new(), Vec::new());
        // The call a published read is called with, for `next`: of the
        // reads named, the first named that was made ready; with none
        // named, the first read made ready whose input the report carries,
        // else the first. An input the report does not carry is shown as
        // an example of every required input (package 5.6c).
        let mut made: Vec<(String, Option<Json>)> = Vec::new();
        for plan in &plans {
            let Plan::Make(drafted, source, done) = plan else {
                continue;
            };
            match make_ready(kernel, &call, &target, drafted, (source.as_ref(), *done)).await {
                Ok(entry) => {
                    let input = match source {
                        Some(Source::Report(input)) => Some(input.clone()),
                        _ => None,
                    };
                    made.push((drafted.tool.as_str().to_owned(), input));
                    ready.push(entry);
                }
                Err(entry) => failed.push(entry),
            }
        }
        let first_call: Option<(String, Json)> = match &named {
            Some(named) => named
                .iter()
                .find_map(|name| made.iter().find(|(tool, _)| tool == name)),
            None => made
                .iter()
                .find(|(_, input)| input.is_some())
                .or_else(|| made.first()),
        }
        .map(|(tool, input)| {
            let input = input.clone().unwrap_or_else(|| {
                compiled
                    .compiled_target
                    .tools
                    .iter()
                    .find(|compiled| compiled.name.as_str() == tool)
                    .and_then(|compiled| {
                        serde_json::from_slice::<Json>(compiled.input_schema.as_bytes()).ok()
                    })
                    .map_or_else(|| json!({}), |schema| super::tools::example_input(&schema))
            });
            (tool.clone(), input)
        });

        // The publish gates, as `test.run` runs them.
        let mut gates_input = json!({"target": target.as_str()});
        if let Some(named) = &named {
            gates_input["tools"] = json!(named);
        }
        let tested = step(kernel, &call, "test.run", gates_input).await;
        let gates = tested.result.clone().unwrap_or(Json::Null);
        let passed = tested.ok && gates["passed"] == true;
        let mut shown = json!({
            "target": target.as_str(),
            "ready": ready,
            "left_alone": left,
            "failed": failed,
            "gates": {
                "passed": passed,
                "missing": gates.get("missing").cloned().unwrap_or_else(|| json!([])),
            },
        });
        if !passed {
            let publish = json!({"state": "not_asked"});
            shown["text"] = json!(summary(
                &ready_of(&shown),
                &left,
                &failed_of(&shown),
                &publish
            ));
            shown["publish"] = publish;
            shown["state"] = json!("not_asked");
            // The drafts with no offline test, when each is one left alone:
            // the gate stays whole, and removing them is one way on.
            let missing: Vec<String> = shown["gates"]["missing"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|entry| entry["file"].as_str().map(str::to_owned))
                .collect();
            let next = if !failed_of(&shown).is_empty() {
                Next::new(
                    "draft.ready",
                    json!({"target": target.as_str()}),
                    "after correcting what each entry under `failed` names in its `error.fix`: what was done stays done",
                )
            } else if !missing.is_empty() {
                Next::new(
                    "draft.delete",
                    json!({"target": target.as_str(), "files": missing}),
                    "to publish without the drafts that have no offline test; or make each ready as its entry under `left_alone` says, and call `draft.ready` again",
                )
            } else {
                tested.next.clone().unwrap_or_else(|| {
                    Next::new(
                        "test.run",
                        json!({"target": target.as_str()}),
                        "to see which gate did not pass",
                    )
                })
            };
            let failure = as_failure(&tested).unwrap_or_else(|| {
                codes::input(
                    codes::SCHEMA_INVALID,
                    "The publish gates did not pass.",
                    "Call `test.run` to see which gate did not pass.",
                )
            });
            return Ok(Output::ok(shown).failed(failure).then(next));
        }

        // A person is asked to publish, as `publish.request` asks.
        let mut input = json!({"target": target.as_str()});
        if let Some(version) = call.text("version") {
            input["version"] = json!(version);
        }
        if let Some(named) = &named {
            input["tools"] = json!(named);
        }
        let asked = step(kernel, &call, "publish.request", input).await;
        let answer = asked.result.clone().unwrap_or(Json::Null);
        let publish = match (asked.exit, &asked.error) {
            (ExitClass::PersonNeeded, None) => {
                let mut publish = Map::new();
                publish.insert("state".to_owned(), json!("needs_person"));
                for key in ["link", "person", "act", "expires_at"] {
                    if let Some(value) = answer.get(key) {
                        publish.insert(key.to_owned(), value.clone());
                    }
                }
                publish.insert("tools".to_owned(), json!(published));
                Json::Object(publish)
            }
            (ExitClass::Succeeded, None) => json!({
                "state": "published",
                "version": answer["version"],
                "tools": answer.get("tools").cloned().unwrap_or_else(|| json!(published)),
            }),
            _ => json!({"state": "not_asked", "error": step_failure(&asked)}),
        };
        shown["text"] = json!(summary(
            &ready_of(&shown),
            &left,
            &failed_of(&shown),
            &publish
        ));
        let state = publish["state"].as_str().unwrap_or("").to_owned();
        shown["publish"] = publish;
        shown["state"] = json!(state);
        let called = |when: &str| match &first_call {
            Some((tool, input)) => {
                let when = if super::tools::holds_placeholder(input) {
                    format!("{when}; with a value of yours in place of each `<...>`")
                } else {
                    when.to_owned()
                };
                Next::new("tool.call", json!({"tool": tool, "input": input}), &when)
            }
            None => Next::new(
                "tool.search",
                json!({"target": target.as_str()}),
                "to see the published tools",
            ),
        };
        Ok(match state.as_str() {
            "needs_person" => Output::ok(shown)
                .exit(ExitClass::PersonNeeded)
                .then(called(
                    "after the person has carried out the publish on the page at `publish.link`; until then there is nothing to do",
                )),
            "published" => Output::ok(shown).then(called("now: the version is published")),
            _ => {
                let failure = as_failure(&asked).unwrap_or_else(|| {
                    codes::input(
                        codes::SCHEMA_INVALID,
                        "The publish was not asked for.",
                        "Call `publish.request` to see why.",
                    )
                });
                let next = asked.next.clone().unwrap_or_else(|| {
                    Next::new(
                        "publish.request",
                        json!({"target": target.as_str()}),
                        "to see why the publish was not asked for",
                    )
                });
                Output::ok(shown).failed(failure).then(next)
            }
        })
    }
);

fn ready_of(shown: &Json) -> Vec<Json> {
    shown["ready"].as_array().cloned().unwrap_or_default()
}

fn failed_of(shown: &Json) -> Vec<Json> {
    shown["failed"].as_array().cloned().unwrap_or_default()
}
