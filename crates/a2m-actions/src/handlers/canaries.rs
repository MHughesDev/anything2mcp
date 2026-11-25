//! Canaries on a schedule of their own, apart from a watch (decision 0482).
//!
//! The scheduler keeps the canary and says when its check is due; this
//! module holds what it cannot: an agent asks, and only the person the
//! canary acts for confirms it, on the engine's page, shown exactly what it
//! will send; what it reads, where, and how many requests a check sends come
//! from the published compiled target, never from the caller's input; every
//! check is a run of that read for that person, of the job kind `canary`,
//! whose drift keeps a mark and whose pass lifts one (decision 0380); and a
//! check ends with what its own run ended with.

use std::time::Duration;

use a2m_core::{
    EffectClass, Environment, Failure, FailureClass, Identity, OperationName, PrincipalId,
    RunState, Timestamp, Values,
};
use a2m_engine::{decode_values, encode_values};
use a2m_ledger::{BlobKind, Durability, NewBlob, NewRun, RetentionClass, ToolKind};
use a2m_sched::{
    CanaryAdmission, CanaryEnd, CanaryId, CanaryOutcome, CanaryRow, CanaryState, DueCanary,
    JobKind, JobRequirements, Priority, RateLimit, Scheduler, SessionLimits, Submission,
};
use a2m_vault::OwnerRef;
use serde_json::{Map, Value as Json, json};

use super::handler;
use super::watches::{
    checked_input, failure_json, millis, operation_of, person_of, planned, shown_value,
};
use crate::codes;
use crate::dispatch::{Call, Caller, ExitClass, Next, Output};
use crate::kernel::Kernel;
use crate::runner::{CallerFacts, keep_run_call};

/// The most canaries one call of `canary.list` returns.
const LIST_PAGE: usize = 1000;

fn no_such_canary() -> Failure {
    codes::input(
        codes::NOT_FOUND,
        "There is no such canary of the caller's person.",
        "Call `canary.list` to see the canaries of your person.",
    )
}

fn canary_id(call: &Call) -> Result<CanaryId, Failure> {
    call.required("canary_id")?
        .parse::<CanaryId>()
        .map_err(|_| {
            codes::input(
                codes::SCHEMA_INVALID,
                "That is not a canary's identifier.",
                "Pass the `canary_id` that `canary.create` gave.",
            )
        })
}

fn input_of(text: &Map<String, Json>) -> Result<CanaryId, Failure> {
    text.get("canary_id")
        .and_then(Json::as_str)
        .ok_or_else(no_such_canary)?
        .parse::<CanaryId>()
        .map_err(|_| no_such_canary())
}

/// A canary as every `canary.*` action returns it. It holds no value a
/// check read and not the input.
pub(crate) fn canary_json(row: &CanaryRow) -> Json {
    let mut map = Map::new();
    map.insert("canary_id".to_owned(), json!(row.id.to_string()));
    map.insert("tool".to_owned(), json!(row.operation.as_str()));
    map.insert("target".to_owned(), json!(row.target.as_str()));
    map.insert("environment".to_owned(), json!(row.environment.as_str()));
    map.insert(
        "compiled_target_hash".to_owned(),
        json!(row.target_version.to_string()),
    );
    map.insert("state".to_owned(), json!(row.state.as_str()));
    if let Some(ended) = row.ended {
        map.insert("ended".to_owned(), json!(ended.as_str()));
    }
    map.insert("interval_seconds".to_owned(), json!(row.interval.as_secs()));
    map.insert("ends_at".to_owned(), millis(row.ends_at));
    map.insert("asked_at".to_owned(), millis(row.asked_at));
    if let Some((_, at)) = row.confirmed {
        map.insert("confirmed_at".to_owned(), millis(at));
    }
    if let Some(next) = row.next_due_at()
        && next < row.ends_at
    {
        map.insert("next_due_at".to_owned(), millis(next));
    }
    map.insert("checks".to_owned(), json!(row.checks));
    map.insert("missed".to_owned(), json!(row.missed));
    if let Some(last) = &row.outcome {
        map.insert("outcome".to_owned(), json!(last.outcome.as_str()));
        if let Some(failure) = &last.failure {
            map.insert("failure".to_owned(), failure_json(failure));
        }
        map.insert("outcome_at".to_owned(), millis(last.at));
    }
    map.insert("surface".to_owned(), json!(row.surface.as_str()));
    map.insert(
        "requests_per_check".to_owned(),
        json!(row.requests_per_check),
    );
    Json::Object(map)
}

/// What a canary's last check found, from its run as it ended or rested: a
/// pass, a drift, another failure, or a rest waiting for a person.
fn outcome_of(run: &a2m_ledger::RunRow) -> CanaryOutcome {
    match (run.state, &run.failure) {
        (RunState::Completed, _) => CanaryOutcome::Passed,
        (_, Some((FailureClass::Drift, _))) => CanaryOutcome::Drifted,
        (RunState::NeedsLogin | RunState::NeedsInput | RunState::Stopped, _) => {
            CanaryOutcome::Rested
        }
        _ => CanaryOutcome::Failed,
    }
}

impl Kernel {
    /// The canary, when it acts for the caller's person. Any other canary is
    /// answered as no canary at all, whatever role the caller holds.
    async fn own_canary(&self, caller: &Caller, id: CanaryId) -> Result<CanaryRow, Failure> {
        let person = person_of(caller)?;
        let row = self
            .sched()?
            .canary(id)
            .await
            .map_err(|_| no_such_canary())?;
        if row.acts_for != person || row.workspace != caller.principal.workspace {
            return Err(no_such_canary());
        }
        Ok(row)
    }

    /// The rate limit of a canary's surface, as a check's requests are
    /// counted against it: the surface's own declared limit, or the
    /// scheduler's default.
    async fn canary_surface_rate(&self, row: &CanaryRow) -> Result<RateLimit, Failure> {
        let compiled_target = self
            .compiled_target_by_hash(&row.target, &row.target_version)
            .await?;
        let declared = compiled_target
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

    /// Whether a confirmed or running canary keeps a solo engine from
    /// stopping when it is idle. Every canary ends at its end, so no hold
    /// lasts past it.
    pub(crate) async fn canaries_hold_the_engine(&self) -> Result<bool, Failure> {
        let now = self.inner.clock.now();
        self.store()?
            .read(move |tx| {
                Ok(a2m_sched::canary::open(tx)?.iter().any(|canary| {
                    matches!(canary.state, CanaryState::Confirmed | CanaryState::Running)
                        && canary.ends_at > now
                }))
            })
            .await
    }

    /// One pass over the canaries, after the watches': ends each past its
    /// end or under another published version; admits each check that is
    /// due as a read run of the kind `canary`; and ends each check whose run
    /// has ended or rests, with what that run ended with. Every failure is
    /// left for the next pass.
    pub(crate) async fn tend_canaries(&self) {
        let (Ok(sched), Ok(store)) = (self.sched().cloned(), self.store().cloned()) else {
            return;
        };
        let mut changed = false;
        if let Ok(due) = sched.due_canaries().await {
            for check in due {
                if self.submit_canary_check(&sched, &check).await {
                    changed = true;
                }
            }
        }
        let running = store
            .read(|tx| {
                let mut out = Vec::new();
                for canary in a2m_sched::canary::open(tx)? {
                    let Some(run) = canary.running_run else {
                        continue;
                    };
                    let row = tx.ledger().run(run)?;
                    let working = Scheduler::unfinished_job(tx, run)?.is_some();
                    out.push((canary.id, row, working));
                }
                Ok(out)
            })
            .await
            .unwrap_or_default();
        for (canary, run, working) in running {
            let resting = matches!(
                run.state,
                RunState::NeedsLogin | RunState::NeedsInput | RunState::Stopped
            ) && !working;
            if !run.state.is_final() && !resting {
                continue;
            }
            let outcome = outcome_of(&run);
            if sched
                .end_canary_check(canary, run.id, outcome, run.failure.clone())
                .await
                .is_ok()
            {
                changed = true;
            }
        }
        if changed {
            self.run_changed();
        }
    }

    /// Admits one due check as a run of the canary's own read, for its own
    /// person, with its own stored input. Whether it was begun.
    async fn submit_canary_check(&self, sched: &Scheduler, check: &DueCanary) -> bool {
        let canary = &check.canary;
        // The surface's own declared budget is set before the check is
        // admitted, as for a watch's check.
        if let Ok(compiled_target) = self
            .compiled_target_by_hash(&canary.target, &canary.target_version)
            .await
            && let Some(rate) = compiled_target
                .surfaces
                .iter()
                .find(|surface| surface.name == canary.surface)
                .and_then(|surface| surface.rate)
            && let Ok(key) = a2m_sched::BudgetKey::surface(
                canary.workspace,
                &canary.target,
                &canary.environment,
                canary.surface.as_str(),
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
            .submit_canary(check, self.canary_submission(canary))
            .await
        {
            Ok(CanaryAdmission::Started { .. }) => true,
            Ok(CanaryAdmission::Deferred { .. }) => false,
            // A refusal the scheduler does not wait out is tried again
            // after a minute at most, and never at once.
            Err(_) => {
                let until = Timestamp::from_unix_millis(
                    self.inner
                        .clock
                        .now()
                        .as_unix_millis()
                        .saturating_add(60_000),
                );
                let _ = sched.defer_canary(canary.id, until).await;
                false
            }
        }
    }

    /// The submission of a check of `canary`: a run of its own read, for its
    /// own person, with its own stored input, as the lowest work, of the job
    /// kind `canary`, which writes nothing.
    fn canary_submission(&self, canary: &CanaryRow) -> Submission {
        let run = NewRun {
            workspace: canary.workspace,
            principal: canary.acts_for,
            client_label: canary.id.to_string(),
            tool_name: canary.operation.clone(),
            tool_kind: ToolKind::Operation,
            target: canary.target.clone(),
            target_version: canary.target_version,
            environment: canary.environment.clone(),
            identity: Some(Identity::Person(canary.acts_for)),
            effect: EffectClass::Read,
            input_hash: canary.input_hash,
            input: canary.input,
            retention: RetentionClass::Long,
        };
        Submission {
            job: JobRequirements {
                kind: JobKind::Canary,
                target: canary.target.clone(),
                environment: canary.environment.clone(),
                modes: Vec::new(),
                os: None,
                acts_as: Some(canary.acts_for),
                session: None,
                session_limits: SessionLimits {
                    reads: 4,
                    writes: 1,
                },
                exclusive: Vec::new(),
                max_step: Duration::from_secs(30),
                writes: false,
                verify_after_loss: false,
            },
            run,
            priority: Priority::Batch,
            health: super::tools::sched_health(self),
            queue_wait: None,
            budgets: Vec::new(),
        }
    }

    /// What a person reads on the page of a canary's confirmation, beside
    /// its identifier: the read, its input, the interval and the end, how
    /// many checks and requests that is, what its drift does, and what is
    /// kept (decision 0482).
    pub(crate) async fn canary_confirmation_lines(
        &self,
        caller: &Caller,
        input: &Map<String, Json>,
    ) -> Result<Vec<String>, Failure> {
        let row = self.own_canary(caller, input_of(input)?).await?;
        if row.state != CanaryState::Asked {
            return Err(codes::input(
                codes::SCHEMA_INVALID,
                format!(
                    "The canary is `{}`, so there is nothing to confirm.",
                    row.state.as_str()
                ),
                "Call `canary.get` to see where it stands.",
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
        let rate = self.canary_surface_rate(&row).await?;
        let per_day = u128::from(row.requests_per_check) * 86_400_000
            / u128::try_from(interval_ms).unwrap_or(1).max(1);
        let allowed = u128::from(rate.refill) * 86_400_000 / rate.per.as_millis().max(1);
        let share = self.sched()?.settings().watch.budget_share_percent;
        Ok(vec![
            format!(
                "The read: `{}` of the target `{}`, in the environment `{}`, as published with the compiled target {}. It reads and never writes; a canary is never given a step that writes.",
                row.operation, row.target, row.environment, row.target_version
            ),
            if shown.is_empty() {
                "Its input: none.".to_owned()
            } else {
                format!("Its input, fixed for the life of the canary: {shown}.")
            },
            format!(
                "Every {} seconds, until {} milliseconds after 1970 in UTC, about {} hours from now: at most {checks} checks. A due time missed while the engine is not running is counted and not made up. A new published version of the target ends the canary.",
                row.interval.as_secs(),
                row.ends_at.as_unix_millis(),
                lasts_ms / 3_600_000
            ),
            format!(
                "Each check sends up to {} requests to the surface `{}`: about {per_day} a day of the {allowed} it allows. All watches and canaries on the surface together may take at most {share} in every hundred.",
                row.requests_per_check, row.surface
            ),
            "When a check finds that the system no longer matches what the target knows of it, every tool that relies on the same knowledge stops before it sends anything, until a check passes again or a repaired version is published.".to_owned(),
            "While it runs, each check uses your session for the target. A check that would need you to sign in is let go; it never asks you itself.".to_owned(),
            "What is kept: for each canary, how many checks it made and what the last one found, never a value; the values stay in each check's run, as for any read you call.".to_owned(),
        ])
    }
}

handler!(
    /// `canary.create`: an agent asks for a canary. It is kept as asked, and
    /// its person is shown the confirmation on the engine's page; nothing is
    /// sent until they confirm it there (decision 0482).
    canary_create,
    |kernel, call| {
        let name = OperationName::new(call.required("tool")?)?;
        let found = kernel.find_tool(&name).await?;
        let operation = operation_of(&found)?;
        if operation.effect != EffectClass::Read {
            return Err(codes::policy(
                a2m_sched::codes::CANARY_NOT_A_READ,
                format!("`{name}` is not a read, and a canary sends reads alone."),
                "Ask for a canary of an operation whose effect is `read`.",
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
        let environment = match call.text("environment") {
            Some(given) => Environment::new(given)?,
            None => found
                .compiled_target
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
        if found
            .compiled_target
            .environment_named(&environment)
            .is_none()
        {
            return Err(codes::input(
                codes::NOT_FOUND,
                format!(
                    "The target `{}` has no environment named `{environment}`.",
                    found.target
                ),
                "Leave `environment` out to use the target's default.",
            ));
        }
        let planned = planned(&found.compiled_target, operation)?;
        let interval = Duration::from_secs(
            u64::try_from(call.int("interval_seconds").unwrap_or(0)).unwrap_or(0),
        );
        let ends_at = Timestamp::from_unix_millis(call.int("ends_at").unwrap_or(0));
        let principal = &call.caller.principal;
        let store = kernel.store()?;
        // The input is stored as a call's is, and the run's call is kept
        // now, under the canary's person: every check is a run of this read
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
            target_version: found.compiled_target_hash,
            environment: environment.clone(),
            identity: Some(Identity::Person(person)),
            effect: EffectClass::Read,
            input_hash,
            input: Some(stored),
            retention: RetentionClass::Long,
        };
        let facts = CallerFacts::of(&call.caller, call.decision).with_project(call.project.clone());
        store
            .write(Durability::Synced, move |tx| {
                keep_run_call(tx, &kept, stored, &facts)
            })
            .await?;
        let row = kernel
            .sched()?
            .ask_canary(a2m_sched::NewCanary {
                workspace: principal.workspace,
                target: found.target.clone(),
                environment,
                operation: name.clone(),
                target_version: found.compiled_target_hash,
                effect: operation.effect,
                input_hash,
                input: Some(stored),
                surface: planned.surface,
                requests_per_check: planned.requests_per_check,
                interval,
                ends_at,
                asked_by: principal.principal.id,
                acts_for: person,
            })
            .await?;
        // The confirmation is the person's, on the engine's page, exactly
        // as a watch's is (decision 0283).
        let def = kernel
            .actions()
            .find("canary.confirm")
            .cloned()
            .ok_or_else(|| {
                codes::input(
                    codes::ACTION_NOT_BUILT,
                    "`canary.confirm` is not in this engine's registry.",
                    "Call `action.search` to see which actions are built.",
                )
            })?;
        let mut asked = Map::new();
        asked.insert("canary_id".to_owned(), json!(row.id.to_string()));
        let project = (call.project.clone(), call.project_refused.clone());
        let mut result = canary_json(&row);
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
                    "The canary waits for its person, who was not shown it on a page: {} The person confirms it with `canary.confirm` at a terminal.",
                    failure.message()
                )),
        };
        Ok(output.then(Next::new(
            "canary.get",
            json!({"canary_id": row.id.to_string()}),
            "after the person has confirmed the canary on its page; until then nothing is sent",
        )))
    }
);

handler!(
    /// `canary.confirm`: the canary's person confirms it, with presence. The
    /// dispatcher has asked for the proof, and sends an agent's call to the
    /// page instead; the scheduler holds that only the person the canary
    /// acts for confirms it, for the version it was asked under and that is
    /// published now, within the share of its surface's budget, in one
    /// transaction.
    canary_confirm,
    |kernel, call| {
        if call.presence.is_none() {
            return Err(codes::policy(
                codes::PERSON_REQUIRED,
                "`canary.confirm` is a person's act, and no proof of presence was given.",
                "Confirm the canary on its page, or at a terminal yourself.",
            ));
        }
        let id = canary_id(&call)?;
        let row = kernel.own_canary(&call.caller, id).await?;
        let person: PrincipalId = person_of(&call.caller)?;
        let (workspace, target) = (row.workspace, row.target.clone());
        let current = kernel
            .store()?
            .read(move |tx| tx.versions().current(&workspace, &target))
            .await?
            .ok_or_else(|| {
                codes::policy(
                    a2m_sched::codes::CANARY_VERSION_CHANGED,
                    "The canary's target has no published version now.",
                    "Publish the target, and ask for a new canary.",
                )
            })?;
        let limit = kernel.canary_surface_rate(&row).await?;
        let confirmed = kernel
            .sched()?
            .confirm_canary(id, person, current, limit)
            .await?;
        kernel.run_changed();
        Ok(Output::ok(canary_json(&confirmed)).then(Next::new(
            "canary.get",
            json!({"canary_id": id.to_string()}),
            "to see what its checks find",
        )))
    }
);

handler!(
    /// `canary.get`: one canary of the caller's person.
    canary_get,
    |kernel, call| {
        let row = kernel.own_canary(&call.caller, canary_id(&call)?).await?;
        Ok(Output::ok(canary_json(&row)))
    }
);

handler!(
    /// `canary.list`: the canaries of the caller's person, oldest first.
    canary_list,
    |kernel, call| {
        let person = person_of(&call.caller)?;
        let workspace = call.caller.principal.workspace;
        let open_only = call.flag("open_only").unwrap_or(false);
        let rows = kernel
            .store()?
            .read(move |tx| a2m_sched::canary::of_person(tx, &workspace, &person, open_only))
            .await?;
        let complete = rows.len() <= LIST_PAGE;
        let shown: Vec<Json> = rows.iter().take(LIST_PAGE).map(canary_json).collect();
        Ok(Output::ok(json!({"canaries": shown, "complete": complete})))
    }
);

handler!(
    /// `canary.end`: the canary's person, or the caller that asked for it,
    /// ends it; a check under way is let go.
    canary_end,
    |kernel, call| {
        let id = canary_id(&call)?;
        let workspace = call.caller.principal.workspace;
        let caller = call.caller.principal.principal.id;
        let person = call.caller.principal.person();
        let row = kernel
            .sched()?
            .canary(id)
            .await
            .map_err(|_| no_such_canary())?;
        let why = if row.workspace != workspace {
            return Err(no_such_canary());
        } else if person == Some(row.acts_for) {
            CanaryEnd::EndedByPerson
        } else if caller == row.asked_by {
            CanaryEnd::EndedByCaller
        } else {
            return Err(no_such_canary());
        };
        let ended = kernel.sched()?.end_canary(id, why).await?;
        kernel.run_changed();
        Ok(Output::ok(canary_json(&ended)))
    }
);
