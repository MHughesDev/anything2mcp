//! Watches: one published read, called with one fixed input at a fixed
//! interval until an end, whose checks record what was added, changed, or
//! went out of view (decisions 0234, 0247, 0262, 0283).
//!
//! The ledger keeps the watch, its checks, and its changes, and compares
//! each check with what earlier checks saw; the scheduler says when a check
//! is due and admits it as the lowest work. This module holds what neither
//! can: an agent asks, and only its person confirms, on the engine's page;
//! the person a watch acts for is the caller's own; what a watch reads,
//! where, how many requests a check sends, and how its result is compared
//! all come from the published plan, never from the caller's input; every
//! check ends with its own run's stored result; and a change names a record
//! only from a run of the caller's own person.

use std::collections::BTreeSet;
use std::time::Duration;

use a2m_core::{
    BindingStatus, Digest, EffectClass, Environment, Failure, FailureClass, FailureCode, FieldOf,
    Identity, OperationName, OperationPlan, Plan, PlanExpr, PrincipalId, RecordKey, RunId,
    RunState, SessionState, Timestamp, ToolRuns, Value, ValueType, Values,
};
use a2m_engine::{decode_values, encode_values};
use a2m_ledger::{
    BlobKind, ChangeRow, Durability, NewBlob, NewRun, RetentionClass, ToolKind, Tx, WatchId,
    WatchKey, WatchReason, WatchRow, WatchShape, WatchState,
};
use a2m_sched::{
    CheckAdmission, DueCheck, JobKind, JobRequirements, Priority, RateLimit, Scheduler,
    SessionLimits, Submission,
};
use a2m_vault::OwnerRef;
use serde_json::{Map, Value as Json, json};

use super::handler;
use crate::codes;
use crate::dispatch::{Call, Caller, ExitClass, Next, Output};
use crate::kernel::Kernel;
use crate::runner::{CallerFacts, keep_run_call};
use crate::tools::FoundTool;

/// The most changes one call of `watch.changes` returns.
const CHANGES_PAGE: u32 = 100;

/// The most watches one call of `watch.list` returns. The most a person may
/// have open is a setting of at most 1000.
const LIST_PAGE: usize = 1000;

fn no_such_watch() -> Failure {
    codes::input(
        codes::NOT_FOUND,
        "There is no such watch of the caller's person.",
        "Call `watch.list` to see the watches of your person.",
    )
}

fn watch_id(call: &Call) -> Result<WatchId, Failure> {
    call.required("watch_id")?.parse::<WatchId>().map_err(|_| {
        codes::input(
            codes::SCHEMA_INVALID,
            "That is not a watch's identifier.",
            "Pass the `watch_id` that `watch.create` gave.",
        )
    })
}

/// The person a caller acts for: the only person whose watches it may ask
/// for, see, or change (decision 0283). Never taken from the input.
fn person_of(caller: &Caller) -> Result<PrincipalId, Failure> {
    caller.principal.person().ok_or_else(|| {
        codes::policy(
            codes::PERSON_REQUIRED,
            "A watch acts for a person, and this caller acts for none.",
            "Call as a person, or as an agent acting for one.",
        )
    })
}

/// Whether a check's run is left to be cancelled, once the checks in
/// `stopping` stop waiting on it: a check of a watch opened it, and no check
/// under way other than those names it (decisions 0285 and 0287). A run no
/// check opened, such as the person's own call that a check joined, is never
/// cancelled for a watch; a run another watch's check still rests on is left
/// for that check, and the last check on it to stop cancels it.
/// The ledger answers the rule in one place, for this module and for the
/// scheduler's end of watches past their end.
fn run_left_to_cancel(
    tx: &mut Tx<'_>,
    run: RunId,
    stopping: &BTreeSet<(WatchId, u64)>,
) -> Result<bool, Failure> {
    tx.watches().run_left_to_cancel(run, stopping)
}

fn failure_json((class, code): &(FailureClass, FailureCode)) -> Json {
    json!({"class": class.as_str(), "code": code.as_str()})
}

fn millis(at: Timestamp) -> Json {
    json!(at.as_unix_millis())
}

/// A watch as every `watch.*` action returns it. It holds no value a check
/// read and not the input: the caller gave the input, and the values are in
/// each check's run.
pub(crate) fn watch_json(row: &WatchRow) -> Json {
    let mut map = Map::new();
    map.insert("watch_id".to_owned(), json!(row.id.to_string()));
    map.insert("tool".to_owned(), json!(row.operation.as_str()));
    map.insert("target".to_owned(), json!(row.target.as_str()));
    map.insert("environment".to_owned(), json!(row.environment.as_str()));
    map.insert(
        "plan_hash".to_owned(),
        json!(row.target_version.to_string()),
    );
    map.insert("state".to_owned(), json!(row.state.as_str()));
    if let Some(reason) = row.reason {
        map.insert("reason".to_owned(), json!(reason.as_str()));
    }
    if let Some(failure) = &row.reason_failure {
        map.insert("failure".to_owned(), failure_json(failure));
    }
    map.insert("interval_seconds".to_owned(), json!(row.interval.as_secs()));
    map.insert("ends_at".to_owned(), millis(row.ends_at));
    map.insert("asked_at".to_owned(), millis(row.asked_at));
    if let Some((_, at)) = row.confirmed {
        map.insert("confirmed_at".to_owned(), millis(at));
        if let Some(phase) = row.phase
            && matches!(row.state, WatchState::Confirmed | WatchState::Paused)
        {
            let next = a2m_sched::due_at(at, phase, row.interval, row.next_due_index);
            if next < row.ends_at {
                map.insert("next_due_at".to_owned(), millis(next));
            }
        }
    }
    map.insert("checks".to_owned(), json!(row.checks));
    map.insert("surface".to_owned(), json!(row.surface.as_str()));
    map.insert(
        "requests_per_check".to_owned(),
        json!(row.requests_per_check),
    );
    map.insert(
        "compared".to_owned(),
        json!(match row.shape {
            WatchShape::Whole => "whole",
            WatchShape::List { .. } => "records",
        }),
    );
    map.insert(
        "ignore".to_owned(),
        json!(row.ignore.iter().collect::<Vec<_>>()),
    );
    if let Some(condition) = &row.condition {
        map.insert("condition".to_owned(), json!(condition.source));
    }
    Json::Object(map)
}

/// What a watch of `operation` reads, as the published plan states it: the
/// surface of its first verified binding, the requests a check sends there,
/// and how its result is compared. Nothing of it comes from a caller.
struct Planned {
    surface: a2m_core::SurfaceName,
    requests_per_check: u32,
    shape: WatchShape,
}

/// The surface, cost, and shape of a watch of `operation` in `plan`.
///
/// The shape is the whole result. A plan declares no list of records with
/// a key: an operation's output is a value of one field type or a list of
/// them (decision 0283), so there are no records to compare one by one.
fn planned(plan: &Plan, operation: &OperationPlan) -> Result<Planned, Failure> {
    let binding = operation
        .bindings
        .iter()
        .find(|binding| binding.status == BindingStatus::Verified)
        .ok_or_else(|| {
            codes::input(
                codes::SCHEMA_INVALID,
                format!(
                    "`{}` has no verified binding, so a watch could not run it.",
                    operation.name
                ),
                "Promote a binding of the read, publish, and ask again.",
            )
        })?;
    let surface = plan.surface(binding.surface).ok_or_else(|| {
        codes::input(
            codes::SCHEMA_INVALID,
            "The read's binding names a surface its plan does not hold.",
            "Publish the target again from its sources.",
        )
    })?;
    // Each step a check carries out sends at most one request: the steps
    // that do the work and those that see the system accept it. A read is
    // not read back.
    let sends = binding.act.len().saturating_add(binding.confirm.len());
    let requests_per_check = u32::try_from(sends).unwrap_or(u32::MAX).max(1);
    Ok(Planned {
        surface: surface.name.clone(),
        requests_per_check,
        shape: WatchShape::Whole,
    })
}

/// The operation a found tool runs, when it is an operation.
fn operation_of(found: &FoundTool) -> Result<&OperationPlan, Failure> {
    match found.tool.runs {
        ToolRuns::Operation(index) => found.plan.operation(index).ok_or_else(|| {
            codes::input(
                codes::SCHEMA_INVALID,
                "The tool's plan does not hold what it runs.",
                "Publish the target again from its sources.",
            )
        }),
        ToolRuns::Pipeline(_) => Err(codes::input(
            codes::SCHEMA_INVALID,
            format!(
                "`{}` is a pipeline, and only an operation can be watched today.",
                found.tool.name
            ),
            "Watch one of the reads the pipeline calls.",
        )),
    }
}

/// Checks a watch's input against the read's declared inputs: every name
/// given is an input, and every input that must be given is. Its values are
/// checked by type when each check runs, as for any call.
fn checked_input(operation: &OperationPlan, input: &Values) -> Result<(), Failure> {
    let mut problems = Vec::new();
    for name in input.names() {
        if !operation
            .inputs
            .iter()
            .any(|declared| declared.name == name)
        {
            problems.push(format!("`{name}` is not an input"));
        }
    }
    for declared in &operation.inputs {
        let given = input
            .get(&declared.name)
            .is_some_and(|value| !value.is_null());
        if declared.required && declared.default.is_none() && !given {
            problems.push(format!("`{}` is required", declared.name));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(codes::input(
            codes::SCHEMA_INVALID,
            format!("The watch's input was refused: {}.", problems.join("; ")),
            "Call `tool.describe` for the read's input, and ask again.",
        ))
    }
}

/// Checks each name left out of the comparison against the read's outputs.
fn checked_ignore(
    operation: &OperationPlan,
    ignore: &[String],
) -> Result<BTreeSet<String>, Failure> {
    let unknown: Vec<&String> = ignore
        .iter()
        .filter(|name| !operation.outputs.iter().any(|output| &output.name == *name))
        .collect();
    if !unknown.is_empty() {
        return Err(codes::input(
            codes::SCHEMA_INVALID,
            format!(
                "`ignore` names what is not an output of the read: {}.",
                unknown
                    .iter()
                    .map(|name| format!("`{}`", codes::shown(name)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            "Name outputs of the read, as `tool.describe` lists them.",
        ));
    }
    Ok(ignore.iter().cloned().collect())
}

/// Checks and compiles a watch's condition: an expression over the read's
/// result, `result`, a record of the read's outputs with the types the plan
/// gives them, that gives true or false (decisions 0234 and 0291). It reads
/// no input, no environment, and nothing but the one result, so the same
/// result always gives the same answer. Kept in the plan's form, which the
/// ledger loads again and evaluates on each check's own result.
fn checked_condition(
    operation: &OperationPlan,
    given: Option<&str>,
) -> Result<Option<PlanExpr>, Failure> {
    let Some(source) = given.filter(|source| !source.trim().is_empty()) else {
        return Ok(None);
    };
    let mut env = a2m_expr::TypeEnv::new(a2m_expr::LibraryVersion::CURRENT);
    let result = ValueType::Record(
        operation
            .outputs
            .iter()
            .map(|output| FieldOf::required(output.name.clone(), output.ty.clone()))
            .collect(),
    );
    env.declare(a2m_expr::names::RESULT, result)?;
    let compiled = a2m_expr::compile(source, &env, Some(&ValueType::Bool))?;
    Ok(Some(compiled.to_plan_expr()?))
}

impl Kernel {
    /// The watch, when it acts for the caller's person. Any other watch is
    /// answered as no watch at all, whatever role the caller holds: a watch
    /// reads under its person's session.
    async fn own_watch(&self, call: &Call, id: WatchId) -> Result<WatchRow, Failure> {
        let person = person_of(&call.caller)?;
        let workspace = call.caller.principal.workspace;
        let row = self
            .store()?
            .read(move |tx| tx.watches().get(id))
            .await
            .map_err(|_| no_such_watch())?;
        if row.workspace == workspace && row.acts_for == person {
            Ok(row)
        } else {
            Err(no_such_watch())
        }
    }

    /// The rate limit of a watch's surface, as a check's requests are
    /// counted against it: the surface's own declared limit, or the
    /// scheduler's default.
    async fn surface_rate(&self, row: &WatchRow) -> Result<RateLimit, Failure> {
        let plan = self.plan_by_hash(&row.target, &row.target_version).await?;
        let declared = plan
            .surfaces
            .iter()
            .find(|surface| surface.name == row.surface)
            .and_then(|surface| surface.rate)
            .map(|rate| RateLimit {
                capacity: rate.requests.max(1),
                refill: rate.requests.max(1),
                per: Duration::from_millis(u64::from(rate.per_ms.max(1))),
            });
        Ok(declared.unwrap_or(self.sched()?.settings().default_rate))
    }

    /// Whether a watch keeps a solo engine from stopping when it is idle:
    /// while one is confirmed or running. A paused watch does not hold it,
    /// and every watch ends at its end (decision 0262), so no hold lasts
    /// past it.
    ///
    /// # Errors
    ///
    /// The store's failures.
    pub async fn watches_hold_the_engine(&self) -> Result<bool, Failure> {
        let now = self.inner.clock.now();
        self.store()?
            .read(move |tx| {
                Ok(tx.watches().open()?.iter().any(|watch| {
                    matches!(watch.state, WatchState::Confirmed | WatchState::Running)
                        && watch.ends_at > now
                }))
            })
            .await
    }

    /// One pass over the watches, after the scheduler's sweep: ends every
    /// watch past its end and cancels the runs of the checks that closed;
    /// admits each check that is due as a read run; ends each check whose
    /// run has ended, with that run's own stored result, and cancels a
    /// check's run that rests; and lets a watch paused for a sign-in go on
    /// once its person's session is valid again. Every failure is left for
    /// the next pass: a watch that cannot be looked at now is looked at
    /// then.
    pub(crate) async fn tend_watches(&self) {
        let (Ok(sched), Ok(store)) = (self.sched().cloned(), self.store().cloned()) else {
            return;
        };
        let mut changed = false;
        // 1. What is due, and what a watch's end closed. The scheduler
        // ends each watch past its end and cancels its check's run in one
        // transaction, so a pass that fails leaves both to the next.
        if let Ok(due) = sched.due_watches().await {
            if !due.cancelled.is_empty() {
                changed = true;
            }
            for check in due.checks {
                if self.submit_watch_check(&sched, &check).await {
                    changed = true;
                }
            }
        }
        // 2. Each check whose run ended, including those that ended while
        // the engine was not running.
        let running = store
            .read(|tx| {
                let mut out = Vec::new();
                for watch in tx.watches().open()? {
                    if watch.state != WatchState::Running {
                        continue;
                    }
                    let Some(seq) = watch.running_check else {
                        continue;
                    };
                    let check = tx.watches().check(watch.id, seq)?;
                    let run = tx.ledger().run(check.run)?;
                    let working = Scheduler::unfinished_job(tx, check.run)?.is_some();
                    out.push((watch.id, seq, run, working));
                }
                Ok(out)
            })
            .await
            .unwrap_or_default();
        for (watch, seq, run, working) in running {
            let resting = matches!(
                run.state,
                RunState::NeedsLogin | RunState::NeedsInput | RunState::Stopped
            ) && !working;
            if !run.state.is_final() && !resting {
                continue;
            }
            // The bytes are the run's own stored result, read here and
            // checked again by the ledger against the run's row.
            let bytes = match (run.state, run.result) {
                (RunState::Completed, Some(blob)) => match store.blobs().get(&blob).await {
                    Ok(opened) => Some(opened.expose().to_vec()),
                    Err(_) => continue,
                },
                _ => None,
            };
            let run_id = run.id;
            let ended = store
                .write(Durability::Synced, move |tx| {
                    tx.watches().end_check(watch, seq, bytes.as_deref())?;
                    // A check's run that rests waits for no watch: each
                    // check on it recorded why, and paused if a person is
                    // needed. The run is cancelled after the last check on
                    // it ended, so that every watch resting on it keeps the
                    // reason the run rested for; one opened by no check is
                    // left as it is.
                    if resting {
                        run_left_to_cancel(tx, run_id, &BTreeSet::new())
                    } else {
                        Ok(false)
                    }
                })
                .await;
            if let Ok(cancel) = ended {
                changed = true;
                if cancel {
                    let _ = sched.cancel(run_id).await;
                }
            }
        }
        // 3. A watch paused for a sign-in goes on once its person's session
        // for the target is valid again.
        if let Ok(keeper) = self.keeper() {
            let paused = store
                .read(|tx| {
                    Ok(tx
                        .watches()
                        .open()?
                        .into_iter()
                        .filter(|watch| {
                            watch.state == WatchState::Paused
                                && watch.reason == Some(WatchReason::NeedsLogin)
                        })
                        .collect::<Vec<_>>())
                })
                .await
                .unwrap_or_default();
            for watch in paused {
                let identity = Identity::Person(watch.acts_for);
                let Ok(sessions) = keeper.status(&watch.workspace, Some(&identity)).await else {
                    continue;
                };
                let valid = sessions.iter().any(|session| {
                    session.target == watch.target
                        && session.environment == watch.environment
                        && session.state == SessionState::Valid
                });
                if valid {
                    let id = watch.id;
                    if store
                        .write(Durability::Synced, move |tx| tx.watches().resume(id))
                        .await
                        .is_ok()
                    {
                        changed = true;
                    }
                }
            }
        }
        if changed {
            self.run_changed();
        }
    }

    /// Admits one due check as a run of the watch's own read, for the
    /// watch's own person, with its own stored input: the run's call was
    /// kept when the watch was asked. Whether it was begun.
    async fn submit_watch_check(&self, sched: &Scheduler, check: &DueCheck) -> bool {
        let watch = &check.watch;
        // The surface's own declared budget is set before the check is
        // admitted, as a run's control sets it before its first request:
        // the watches' share at admission is then a share of the limit the
        // check's requests are counted against, and not of the default.
        if let Ok(plan) = self
            .plan_by_hash(&watch.target, &watch.target_version)
            .await
            && let Some(rate) = plan
                .surfaces
                .iter()
                .find(|surface| surface.name == watch.surface)
                .and_then(|surface| surface.rate)
            && let Ok(key) = a2m_sched::BudgetKey::surface(
                watch.workspace,
                &watch.target,
                &watch.environment,
                watch.surface.as_str(),
            )
            && !self.budget_noted(&key)
        {
            let limit = RateLimit {
                capacity: rate.requests.max(1),
                refill: rate.requests.max(1),
                per: Duration::from_millis(u64::from(rate.per_ms.max(1))),
            };
            if sched.set_budget(key.clone(), limit).await.is_ok() {
                self.note_budget(&key);
            }
        }
        match sched
            .submit_check(check, self.check_submission(watch))
            .await
        {
            Ok(CheckAdmission::Started(_)) => true,
            Ok(CheckAdmission::Deferred { .. }) => false,
            // A refusal the scheduler does not wait out, such as a check its
            // ledger refuses, is tried again after a minute at most, and
            // never at once: the tending task does not spin on it.
            Err(_) => {
                if let Ok(store) = self.store() {
                    let id = watch.id;
                    let until = Timestamp::from_unix_millis(
                        self.inner
                            .clock
                            .now()
                            .as_unix_millis()
                            .saturating_add(60_000),
                    );
                    let _ = store
                        .write(Durability::Relaxed, move |tx| tx.watches().defer(id, until))
                        .await;
                }
                false
            }
        }
    }

    /// The submission of a check of `watch`: a run of its own read, for its
    /// own person, with its own stored input, as the lowest work. Its
    /// identity under ledger rule 6 is that of the person's own call of the
    /// same read and input.
    pub(crate) fn check_submission(&self, watch: &WatchRow) -> Submission {
        let run = NewRun {
            workspace: watch.workspace,
            principal: watch.acts_for,
            client_label: watch.id.to_string(),
            tool_name: watch.operation.clone(),
            tool_kind: ToolKind::Operation,
            target: watch.target.clone(),
            target_version: watch.target_version,
            environment: watch.environment.clone(),
            identity: Some(Identity::Person(watch.acts_for)),
            effect: EffectClass::Read,
            input_hash: watch.input_hash,
            input: watch.input,
            retention: watch.retention,
        };
        Submission {
            job: JobRequirements {
                kind: JobKind::Canary,
                target: watch.target.clone(),
                environment: watch.environment.clone(),
                modes: Vec::new(),
                os: None,
                acts_as: Some(watch.acts_for),
                session: None,
                session_limits: SessionLimits {
                    reads: 4,
                    writes: 1,
                },
                exclusive: Vec::new(),
                max_step: Duration::from_secs(30),
                writes: false,
            },
            run,
            priority: Priority::Batch,
            health: super::tools::sched_health(self),
            queue_wait: None,
            budgets: Vec::new(),
        }
    }

    /// After a publish of `target` at `current`: every watch tied to
    /// another version pauses or ends (decision 0262), and the runs of
    /// those whose check is under way are cancelled, each once, when a check
    /// opened it and no check of a watch that goes on rests on it (decision
    /// 0287); their checks end at the next pass, and the watch pauses then.
    pub(crate) async fn watches_after_publish(
        &self,
        target: &a2m_core::TargetName,
        current: Digest,
    ) {
        let (Ok(store), Ok(sched)) = (self.store().cloned(), self.sched().cloned()) else {
            return;
        };
        let Ok(installation) = self.installation() else {
            return;
        };
        let target = target.clone();
        let workspace = installation.workspace;
        let answered = store
            .write(Durability::Synced, move |tx| {
                let running = tx
                    .watches()
                    .version_changed(&workspace, &target, &current)?;
                let mut stopping = BTreeSet::new();
                let mut runs = BTreeSet::new();
                for id in running {
                    let watch = tx.watches().get(id)?;
                    if let Some(seq) = watch.running_check {
                        stopping.insert((id, seq));
                        runs.insert(tx.watches().check(id, seq)?.run);
                    }
                }
                let mut cancel = Vec::new();
                for run in runs {
                    if run_left_to_cancel(tx, run, &stopping)? {
                        cancel.push(run);
                    }
                }
                Ok(cancel)
            })
            .await
            .unwrap_or_default();
        for run in answered {
            let _ = sched.cancel(run).await;
        }
        // The pass after the cancel ends their checks; a version change
        // pauses each such watch when it is asked again then.
        self.inner.wake.notify_one();
    }

    /// What a person reads on the page of a watch's confirmation, beside
    /// its identifier: the read, its input, the interval and the end, how
    /// many checks that is, its share of the surface's budget, and what is
    /// kept (decision 0262).
    pub(crate) async fn watch_confirmation_lines(
        &self,
        caller: &Caller,
        input: &Map<String, Json>,
    ) -> Result<Vec<String>, Failure> {
        let id = input
            .get("watch_id")
            .and_then(Json::as_str)
            .ok_or_else(no_such_watch)?
            .parse::<WatchId>()
            .map_err(|_| no_such_watch())?;
        let person = person_of(caller)?;
        let row = self.store()?.read(move |tx| tx.watches().get(id)).await?;
        if row.acts_for != person || row.workspace != caller.principal.workspace {
            return Err(no_such_watch());
        }
        if row.state != WatchState::Asked {
            return Err(codes::input(
                codes::SCHEMA_INVALID,
                format!(
                    "The watch is `{}`, so there is nothing to confirm.",
                    row.state.as_str()
                ),
                "Call `watch.get` to see where it stands.",
            ));
        }
        let shown = match row.input {
            Some(blob) => {
                let opened = self.store()?.blobs().get(&blob).await?;
                let values = decode_values(opened.expose())?;
                values
                    .iter()
                    .map(|(name, value)| format!("{name} = {}", shown_value(value)))
                    .collect::<Vec<_>>()
                    .join(", ")
            }
            None => String::new(),
        };
        let now = self.inner.clock.now();
        let lasts_ms = row
            .ends_at
            .as_unix_millis()
            .saturating_sub(now.as_unix_millis())
            .max(0);
        let interval_ms = i64::try_from(row.interval.as_millis())
            .unwrap_or(i64::MAX)
            .max(1);
        let checks = lasts_ms / interval_ms;
        let rate = self.surface_rate(&row).await?;
        let per_day = u128::from(row.requests_per_check) * 86_400_000
            / u128::try_from(interval_ms).unwrap_or(1).max(1);
        let allowed = u128::from(rate.refill) * 86_400_000 / rate.per.as_millis().max(1);
        let share = self.sched()?.settings().watch.budget_share_percent;
        Ok(vec![
            format!(
                "The read: `{}` of the target `{}`, in the environment `{}`, as published with the plan {}. It reads and never writes.",
                row.operation, row.target, row.environment, row.target_version
            ),
            if shown.is_empty() {
                "Its input: none.".to_owned()
            } else {
                format!("Its input, fixed for the life of the watch: {shown}.")
            },
            format!(
                "Every {} seconds, until {} milliseconds after 1970 in UTC, about {} hours from now: at most {checks} checks. A due time missed while the engine is not running is recorded as a gap and not made up.",
                row.interval.as_secs(),
                row.ends_at.as_unix_millis(),
                lasts_ms / 3_600_000
            ),
            format!(
                "Each check sends up to {} requests to the surface `{}`: about {per_day} a day of the {allowed} it allows. All watches on the surface together may take at most {share} in every hundred.",
                row.requests_per_check, row.surface
            ),
            match &row.condition {
                Some(condition) => format!(
                    "It also ends at the first check on which this holds of what the read returned: {}. A check on which it cannot be answered is recorded as a gap.",
                    condition.source
                ),
                None => "It has no condition: it ends at its end, or when you or the agent end it.".to_owned(),
            },
            "While it runs, each check uses your session for the target, and the session is kept signed in while you are away. A check that needs you to sign in pauses the watch; it never asks you itself.".to_owned(),
            "What is kept: for each check, keyed digests of what the read returned, never a value; the values stay in each check's run, as for any read you call.".to_owned(),
        ])
    }
}

/// A value of the input as a page shows it: text as itself, anything else
/// as compact JSON. The page redacts and cuts it.
fn shown_value(value: &Value) -> String {
    match value {
        Value::Text(text) => text.clone(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

/// The record a change names, as values of its key, found in the result of
/// the change's check run, and only when that run is one of the caller's
/// own person's (decision 0262). `None` when the change names no record, the
/// run is another person's, or its result does not hold the record.
async fn record_of(
    kernel: &Kernel,
    person: PrincipalId,
    row: &WatchRow,
    change: &ChangeRow,
    run: Option<RunId>,
) -> Option<Json> {
    let (Some(WatchKey::Record(wanted)), Some(run), WatchShape::List { list, key }) =
        (change.key, run, &row.shape)
    else {
        return None;
    };
    let store = kernel.store().ok()?;
    let found = store.read(move |tx| tx.ledger().run(run)).await.ok()?;
    if found.principal != person || found.workspace != row.workspace {
        return None;
    }
    let opened = store.blobs().get(&found.result?).await.ok()?;
    let values = decode_values(opened.expose()).ok()?;
    let Some(Value::List(items)) = values.get(list) else {
        return None;
    };
    let workspace = row.workspace;
    for item in items {
        let Value::Record(fields) = item else {
            continue;
        };
        let Some(parts) = key
            .iter()
            .map(|field| fields.get(field).cloned())
            .collect::<Option<Vec<Value>>>()
        else {
            continue;
        };
        let Ok(record_key) = RecordKey::new(parts.clone()) else {
            continue;
        };
        let named = store
            .read(move |tx| tx.watches().key_of(&workspace, &record_key))
            .await
            .ok()?;
        if named == WatchKey::Record(wanted) {
            return serde_json::to_value(parts).ok();
        }
    }
    None
}

handler!(
    /// `watch.create`: an agent asks for a watch. It is kept as asked, and
    /// its person is shown the confirmation on the engine's page; nothing
    /// runs until they confirm it there (decision 0283).
    watch_create,
    |kernel, call| {
        let name = OperationName::new(call.required("tool")?)?;
        let found = kernel.find_tool(&name).await?;
        let operation = operation_of(&found)?;
        if operation.effect != EffectClass::Read {
            return Err(codes::policy(
                a2m_ledger::codes::WATCH_NOT_A_READ,
                format!(
                    "`{name}` is not a read, and only a read can be watched; a watch never writes."
                ),
                "Watch an operation whose effect is `read`.",
            ));
        }
        let person = person_of(&call.caller)?;
        let input: Values = match call.input.get("input") {
            Some(given) => serde_json::from_value(given.clone()).map_err(|_| {
                codes::input(
                    codes::SCHEMA_INVALID,
                    "The read's input holds a value that cannot be taken.",
                    "Use text, whole numbers, true or false, lists, and maps; write an exact number as text.",
                )
            })?,
            None => Values::new(),
        };
        checked_input(operation, &input)?;
        let ignore: Vec<String> = call
            .input
            .get("ignore")
            .and_then(Json::as_array)
            .map(|names| {
                names
                    .iter()
                    .filter_map(Json::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        let ignore = checked_ignore(operation, &ignore)?;
        let condition = checked_condition(operation, call.text("condition"))?;
        let environment = match call.text("environment") {
            Some(given) => Environment::new(given)?,
            None => found
                .plan
                .default_environment()
                .map(|environment| environment.name.clone())
                .ok_or_else(|| {
                    codes::input(
                        codes::NOT_FOUND,
                        "The read's target declares no environment.",
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
        let planned = planned(&found.plan, operation)?;
        let interval = Duration::from_secs(
            u64::try_from(call.int("interval_seconds").unwrap_or(0)).unwrap_or(0),
        );
        let ends_at = Timestamp::from_unix_millis(call.int("ends_at").unwrap_or(0));
        let principal = &call.caller.principal;
        let store = kernel.store()?;
        // The input is stored as a call's is, and the run's call is kept
        // now, under the watch's person: every check is a run of this read
        // with this input for this person, and the runner reads its call
        // from there.
        let stored = store
            .blobs()
            .put(NewBlob {
                workspace: principal.workspace,
                owner: OwnerRef::of(&Identity::Person(person)),
                kind: BlobKind::Input,
                class: RetentionClass::Long,
                plain: encode_values(&input)?,
            })
            .await?;
        let input_hash = input.digest();
        let kept = NewRun {
            workspace: principal.workspace,
            principal: person,
            client_label: codes::shown(&call.caller.client),
            tool_name: name.clone(),
            tool_kind: ToolKind::Operation,
            target: found.target.clone(),
            target_version: found.plan_hash,
            environment: environment.clone(),
            identity: Some(Identity::Person(person)),
            effect: EffectClass::Read,
            input_hash,
            input: Some(stored),
            retention: RetentionClass::Long,
        };
        let facts = CallerFacts::of(&call.caller, call.decision);
        store
            .write(Durability::Synced, move |tx| {
                keep_run_call(tx, &kept, stored, &facts)
            })
            .await?;
        let row = kernel
            .sched()?
            .ask_watch(a2m_ledger::NewWatch {
                workspace: principal.workspace,
                target: found.target.clone(),
                environment,
                operation: name.clone(),
                target_version: found.plan_hash,
                effect: operation.effect,
                input_hash,
                input: Some(stored),
                ignore,
                surface: planned.surface,
                requests_per_check: planned.requests_per_check,
                shape: planned.shape,
                interval,
                ends_at,
                asked_by: principal.principal.id,
                acts_for: person,
                client_label: codes::shown(&call.caller.client),
                retention: RetentionClass::Long,
                condition,
            })
            .await?;
        // The confirmation is the person's, on the engine's page, exactly
        // as a publish an agent asks for is (decision 0163).
        let def = kernel
            .actions()
            .find("watch.confirm")
            .cloned()
            .ok_or_else(|| {
                codes::input(
                    codes::ACTION_NOT_BUILT,
                    "`watch.confirm` is not in this engine's registry.",
                    "Call `action.search` to see which actions are built.",
                )
            })?;
        let mut asked = Map::new();
        asked.insert("watch_id".to_owned(), json!(row.id.to_string()));
        let project = (call.project.clone(), call.project_refused.clone());
        let mut result = watch_json(&row);
        let output = match kernel.ask_person(&def, &call.caller, asked, project).await {
            Ok(page) => {
                if let (Json::Object(map), Json::Object(page)) = (&mut result, &page.result) {
                    for field in ["act", "link", "person", "expires_at"] {
                        if let Some(value) = page.get(field) {
                            map.insert(field.to_owned(), value.clone());
                        }
                    }
                }
                Output::ok(result).exit(ExitClass::PersonNeeded)
            }
            Err(failure) => Output::ok(result)
                .exit(ExitClass::PersonNeeded)
                .warn(format!(
                    "The watch waits for its person, who was not shown it on a page: {} The person confirms it with `watch.confirm` at a terminal.",
                    failure.message()
                )),
        };
        Ok(output.then(Next::new(
            "watch.changes",
            json!({"watch_id": row.id.to_string(), "wait_seconds": 60}),
            "after the person has confirmed the watch on its page; until then nothing runs",
        )))
    }
);

handler!(
    /// `watch.confirm`: the watch's person confirms it, with presence. The
    /// dispatcher has asked for the proof, and sends an agent's call to the
    /// page instead; the ledger holds that only the person the watch acts
    /// for confirms it, for the version now published, within the share of
    /// its surface's budget, in one transaction (decision 0262).
    watch_confirm,
    |kernel, call| {
        if call.presence.is_none() {
            return Err(codes::policy(
                codes::PERSON_REQUIRED,
                "`watch.confirm` is a person's act, and no proof of presence was given.",
                "Confirm the watch on its page, or at a terminal yourself.",
            ));
        }
        let id = watch_id(&call)?;
        let row = kernel.own_watch(&call, id).await?;
        let person = person_of(&call.caller)?;
        let (workspace, target) = (row.workspace, row.target.clone());
        let current = kernel
            .store()?
            .read(move |tx| tx.versions().current(&workspace, &target))
            .await?
            .ok_or_else(|| {
                codes::policy(
                    a2m_ledger::codes::WATCH_NEEDS_CONFIRMATION,
                    "The watch's target has no published version now.",
                    "Publish the target, and ask for a new watch.",
                )
            })?;
        let limit = kernel.surface_rate(&row).await?;
        let confirmed = kernel
            .sched()?
            .confirm_watch(id, person, current, limit)
            .await?;
        Ok(Output::ok(watch_json(&confirmed)).then(Next::new(
            "watch.changes",
            json!({"watch_id": id.to_string(), "wait_seconds": 60}),
            "to be told what the checks find",
        )))
    }
);

handler!(
    /// `watch.get`: one watch of the caller's person.
    watch_get,
    |kernel, call| {
        let row = kernel.own_watch(&call, watch_id(&call)?).await?;
        Ok(Output::ok(watch_json(&row)))
    }
);

handler!(
    /// `watch.list`: the watches of the caller's person, oldest first.
    watch_list,
    |kernel, call| {
        let person = person_of(&call.caller)?;
        let workspace = call.caller.principal.workspace;
        let open_only = call.flag("open_only").unwrap_or(false);
        let rows = kernel
            .store()?
            .read(move |tx| tx.watches().list(&workspace, &person, open_only))
            .await?;
        let complete = rows.len() <= LIST_PAGE;
        let shown: Vec<Json> = rows.iter().take(LIST_PAGE).map(watch_json).collect();
        Ok(Output::ok(json!({"watches": shown, "complete": complete})))
    }
);

handler!(
    /// `watch.changes`: the changes after a number, waiting for the first.
    /// A change names a record only from a run of the caller's own person.
    watch_changes,
    |kernel, call| {
        let id = watch_id(&call)?;
        let person = person_of(&call.caller)?;
        // Only a watch of the caller's own person is answered.
        kernel.own_watch(&call, id).await?;
        let after = u64::try_from(call.int("after").unwrap_or(0)).unwrap_or(0);
        let limit = u32::try_from(call.int("limit").unwrap_or(i64::from(CHANGES_PAGE)))
            .unwrap_or(CHANGES_PAGE)
            .clamp(1, CHANGES_PAGE);
        let wait =
            Duration::from_secs(u64::try_from(call.int("wait_seconds").unwrap_or(0)).unwrap_or(0));
        let store = kernel.store()?.clone();
        let mut notice = kernel.run_watch();
        let give_up = tokio::time::sleep(wait);
        tokio::pin!(give_up);
        let (row, changes, runs) = loop {
            notice.borrow_and_update();
            let (row, changes, runs) = store
                .read(move |tx| {
                    let row = tx.watches().get(id)?;
                    let changes = tx.watches().changes(id, after, limit.saturating_add(1))?;
                    let mut runs = std::collections::BTreeMap::new();
                    for change in &changes {
                        for seq in [change.check, change.older_check].into_iter().flatten() {
                            if let std::collections::btree_map::Entry::Vacant(entry) =
                                runs.entry(seq)
                            {
                                entry.insert(tx.watches().check(id, seq)?.run);
                            }
                        }
                    }
                    Ok((row, changes, runs))
                })
                .await?;
            if !changes.is_empty() || row.state == WatchState::Ended {
                break (row, changes, runs);
            }
            tokio::select! {
                alive = notice.changed() => {
                    if alive.is_err() {
                        break (row, changes, runs);
                    }
                }
                () = &mut give_up => break (row, changes, runs),
            }
        };
        let complete = changes.len() <= usize::try_from(limit).unwrap_or(usize::MAX);
        let mut shown = Vec::new();
        let mut last = after;
        for change in changes
            .iter()
            .take(usize::try_from(limit).unwrap_or(usize::MAX))
        {
            last = change.seq;
            let mut map = Map::new();
            map.insert("seq".to_owned(), json!(change.seq));
            map.insert("kind".to_owned(), json!(change.kind.as_str()));
            map.insert("at".to_owned(), millis(change.at));
            let run = change.check.and_then(|seq| runs.get(&seq).copied());
            if let Some(seq) = change.check {
                map.insert("check".to_owned(), json!(seq));
            }
            if let Some(run) = run {
                map.insert("run_id".to_owned(), json!(run.to_string()));
            }
            if let Some(seq) = change.older_check {
                map.insert("older_check".to_owned(), json!(seq));
                if let Some(older) = runs.get(&seq) {
                    map.insert("older_run_id".to_owned(), json!(older.to_string()));
                }
            }
            if !change.outputs.is_empty() {
                map.insert("outputs".to_owned(), json!(change.outputs));
            }
            if let Some(reason) = change.reason {
                map.insert("reason".to_owned(), json!(reason.as_str()));
            }
            if let Some(failure) = &change.why {
                map.insert("failure".to_owned(), failure_json(failure));
            }
            if let Some(missed) = change.missed {
                map.insert(
                    "missed".to_owned(),
                    json!({"first": missed.first.as_unix_millis(), "last": missed.last.as_unix_millis(), "count": missed.count}),
                );
            }
            if let Some(record) = record_of(kernel, person, &row, change, run).await {
                map.insert("record".to_owned(), record);
            }
            shown.push(Json::Object(map));
        }
        let mut output = Output::ok(json!({
            "watch_id": id.to_string(),
            "state": row.state.as_str(),
            "changes": shown,
            "after": last,
            "complete": complete,
        }));
        if row.state != WatchState::Ended {
            output = output.then(Next::new(
                "watch.changes",
                json!({"watch_id": id.to_string(), "after": last, "wait_seconds": 60}),
                "to be told of the next changes",
            ));
        }
        Ok(output)
    }
);

handler!(
    /// `watch.pause`: the watch's person pauses it.
    watch_pause,
    |kernel, call| {
        let row = kernel.own_watch(&call, watch_id(&call)?).await?;
        let id = row.id;
        let paused = kernel
            .store()?
            .write(Durability::Synced, move |tx| {
                tx.watches().pause(id, WatchReason::PausedByPerson)
            })
            .await?;
        Ok(Output::ok(watch_json(&paused)))
    }
);

handler!(
    /// `watch.resume`: the watch's person lets it go on.
    watch_resume,
    |kernel, call| {
        let row = kernel.own_watch(&call, watch_id(&call)?).await?;
        let id = row.id;
        let resumed = kernel
            .store()?
            .write(Durability::Synced, move |tx| tx.watches().resume(id))
            .await?;
        kernel.inner.wake.notify_one();
        Ok(Output::ok(watch_json(&resumed)))
    }
);

handler!(
    /// `watch.end`: its person ends it, or the caller that asked for it.
    /// A check under way is closed, and its run cancelled only when a
    /// watch's check opened it and no other check under way rests on it
    /// (decisions 0285 and 0287).
    watch_end,
    |kernel, call| {
        let id = watch_id(&call)?;
        let workspace = call.caller.principal.workspace;
        let caller = call.caller.principal.principal.id;
        let person = call.caller.principal.person();
        let row = kernel
            .store()?
            .read(move |tx| tx.watches().get(id))
            .await
            .map_err(|_| no_such_watch())?;
        let reason = if row.workspace != workspace {
            return Err(no_such_watch());
        } else if person == Some(row.acts_for) {
            WatchReason::EndedByPerson
        } else if caller == row.asked_by {
            WatchReason::EndedByCaller
        } else {
            return Err(no_such_watch());
        };
        let running = row.running_check.map(|seq| (row.id, seq));
        let store = kernel.store()?.clone();
        let (ended, run) = store
            .write(Durability::Synced, move |tx| {
                let run = match running {
                    Some((id, seq)) => Some(tx.watches().check(id, seq)?.run),
                    None => None,
                };
                let ended = tx.watches().end(id, reason)?;
                // The check is closed now: the run is cancelled only when no
                // other check rests on it.
                let cancel = match run {
                    Some(run) if run_left_to_cancel(tx, run, &BTreeSet::new())? => Some(run),
                    _ => None,
                };
                Ok((ended, cancel))
            })
            .await?;
        if let Some(run) = run {
            let _ = kernel.sched()?.cancel(run).await;
        }
        kernel.run_changed();
        Ok(Output::ok(watch_json(&ended)))
    }
);
