//! The findings of the safety audit of 2026-11-17 (decision 0152), each
//! shown against the test application: whose a run is, a stored call that
//! cannot be read, the proof of presence, the protected settings and what
//! the policy engine has in force, and the smaller ones beside them.

use std::sync::atomic::Ordering;

use a2m_actions::{Caller, CloseReason, ExitClass, FrontDoor, HealthState, PresenceProof, Request};
use a2m_core::{FailureClass, PrincipalId, RunId, RunState, Timestamp};
use a2m_ledger::{Durability, Owner};
use a2m_policy::{
    BuiltIn, Channel, LimitCheck, PreApproval, PresenceCheck, PrincipalRef, PublishProof, Role,
    RoleBinding, Scope, Subject,
};
use serde_json::{Value as Json, json};

use super::kernel::engine::{ITEM_UPSERT, ORDER_GET};
use super::kernel::{Options, World, world};
use super::runs::{item, status};
use crate::dispatch::Principal;

fn code_of(response: &a2m_actions::Response) -> (FailureClass, String) {
    let error = response
        .error
        .as_ref()
        .unwrap_or_else(|| panic!("no error in {response:?}"));
    (error.class, error.code.clone())
}

impl World {
    /// The same caller, as the person at a terminal and not their agent.
    pub(super) fn person(&self) -> Caller {
        Caller::new(self.caller.principal.clone(), FrontDoor::Command, false)
    }

    /// Another person of the same workspace.
    pub(super) fn stranger(&self) -> Caller {
        Caller::new(
            Principal {
                principal: PrincipalRef::person(PrincipalId::from_bytes([0x5a; 16])),
                workspace: self.caller.principal.workspace(),
                acting_for: None,
            },
            FrontDoor::Command,
            false,
        )
    }

    pub(super) async fn call_as(
        &self,
        caller: &Caller,
        action: &str,
        input: Json,
    ) -> a2m_actions::Response {
        self.kernel
            .dispatch(Request::new(action, input, caller.clone()))
            .await
    }

    /// Publishes the built-in set of this machine with `change` made to it,
    /// as an administrator with a person present would.
    pub(super) async fn publish(&self, change: impl FnOnce(&mut a2m_policy::PolicyLayers)) {
        let user = self.caller.principal.principal().clone();
        let mut layers = BuiltIn::local(user.id, self.caller.principal.workspace())
            .layers()
            .clone();
        change(&mut layers);
        let policy = self.kernel.policy().unwrap().clone();
        let candidate = policy.compile(&layers).unwrap();
        self.kernel
            .store()
            .unwrap()
            .write(Durability::Synced, move |tx| {
                policy
                    .publish(
                        tx,
                        &candidate,
                        &user,
                        PublishProof {
                            presence: Some(PresenceCheck::OsPrompt),
                        },
                    )
                    .map(|_| ())
            })
            .await
            .unwrap();
    }

    /// Calls a tool without waiting, on a kernel with no runner loop, and
    /// gives the run that is queued.
    async fn queue(&self, tool: &str, input: Json) -> (String, RunId) {
        let queued = self
            .call(
                "tool.call",
                json!({"tool": tool, "input": input, "wait_seconds": 0}),
            )
            .await;
        assert!(queued.result.is_some(), "{queued:?}");
        assert_eq!(status(&queued), "queued", "{queued:?}");
        let run = queued.result.unwrap()["run_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let id: RunId = run.parse().unwrap();
        (run, id)
    }

    /// Changes what the store keeps of one run's call, with one statement
    /// that takes the run's input hash.
    async fn spoil(&self, sql: &'static str, id: RunId) {
        self.kernel
            .store()
            .unwrap()
            .write(Durability::Synced, move |tx| {
                let row = tx.ledger().run(id)?;
                tx.tables(Owner::Actions)
                    .execute(
                        sql,
                        &[a2m_ledger::SqlValue::from(row.input_hash.to_string())],
                    )
                    .map(|_| ())
            })
            .await
            .unwrap();
    }

    /// Asks the policy engine's limiter about a run of `records` records.
    async fn within_limits(&self, records: u32) -> Result<(), a2m_core::Failure> {
        let policy = self.kernel.policy().unwrap().clone();
        let check = LimitCheck {
            principal: self.caller.principal.principal().clone(),
            workspace: self.caller.principal.workspace(),
            records: Some(records),
            count_call: false,
        };
        self.kernel
            .store()
            .unwrap()
            .write(Durability::Synced, move |tx| {
                Ok(policy.check_limits(tx, &check))
            })
            .await
            .unwrap()
    }

    /// The audit log's entries for one action, as `audit.query` gives them.
    async fn audited(&self, action: &str) -> Vec<String> {
        let log = self
            .call_as(&self.person(), "audit.query", json!({"limit": 1000}))
            .await;
        assert!(log.ok, "{:?}", log.error);
        log.result.unwrap()["entries"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|entry| entry["action"] == action)
            .map(|entry| entry["detail"].as_str().unwrap().to_owned())
            .collect()
    }
}

// ---- A1: a run is its person's alone -------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn another_persons_run_is_not_read_resumed_or_cancelled_and_a_resume_keeps_its_caller() {
    let world = World::ready().await;
    // Everyone in the workspace holds the roles the four actions need, so
    // that a second person's call reaches the handler: what refuses it
    // below is whose run it is, and no role.
    world
        .publish(|layers| {
            for role in [Role::Viewer, Role::Operator, Role::Auditor] {
                layers.roles.push(RoleBinding {
                    who: Subject::Anyone,
                    role,
                });
            }
        })
        .await;
    let (run, id) = world.rest_a_write("L-430").await;
    let stranger = world.stranger();
    let not_found = (FailureClass::Input, "not_found".to_owned());

    for action in ["run.get", "run.resume", "run.cancel"] {
        let refused = world
            .call_as(&stranger, action, json!({"run_id": run}))
            .await;
        assert_eq!(code_of(&refused), not_found, "{action}: {refused:?}");
    }
    let events = world
        .kernel
        .events(&stranger.principal, id, 0, 10)
        .await
        .unwrap_err();
    assert_eq!(events.code().as_str(), "not_found");
    // Nothing of it changed: it still waits, and it was not cancelled.
    let row = world.kernel.wait_for_rest(id, std::time::Duration::ZERO);
    assert_eq!(row.await.unwrap().state, RunState::NeedsApproval);
    // Reading a run's evidence is the auditor's role, which the policy set
    // gave: it shows what happened, and resumes nothing.
    let trace = world
        .call_as(&stranger, "trace.show", json!({"run_id": run}))
        .await;
    assert!(trace.ok, "{trace:?}");

    // The run's own person reads it, as their agent does.
    let own = world
        .call_as(&world.person(), "run.get", json!({"run_id": run}))
        .await;
    assert_eq!(status(&own), "needs_approval", "{own:?}");

    // The run was started by the person's agent. The person resumes it at a
    // terminal: what was kept of the run's caller stays as it was kept.
    let kept = || async {
        world
            .kernel
            .store()
            .unwrap()
            .read(move |tx| {
                let row = tx.ledger().run(id)?;
                Ok(crate::runner::run_call(tx, &row)?.map(|call| call.caller))
            })
            .await
            .unwrap()
            .expect("the run's call is kept")
    };
    let before = kept().await;
    assert!(before.is_agent);
    world.approve(id).await;
    let done = world
        .call_as(&world.person(), "run.resume", json!({"run_id": run}))
        .await;
    assert_eq!(status(&done), "completed", "{done:?}");
    assert_eq!(kept().await, before);
    assert_eq!(world.app.write_count("A-102", "L-430").await.unwrap(), 1);
    world.kernel.close(CloseReason::Asked).await;
}

// ---- A5: a stored call that cannot be read -------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_run_whose_stored_call_cannot_be_read_is_ended_and_never_run() {
    // No runner loop yet: the runs stay queued while their calls are spoilt.
    let world = world(Options::default()).await;
    world
        .kernel
        .publish_plan(&super::kernel::engine::plan(&world.app.url("")))
        .await
        .unwrap();
    // The runner says hello and its loop is stopped, so that it is
    // enrolled and claims nothing.
    world
        .kernel
        .start_runner_loop(4)
        .await
        .unwrap()
        .stop()
        .await;
    let world = &world;
    let (unparsed, unparsed_id) = world.queue(ORDER_GET, json!({"order_id": "A-100"})).await;
    let (missing, missing_id) = world.queue(ORDER_GET, json!({"order_id": "A-101"})).await;
    let (write, write_id) = world.queue(ITEM_UPSERT, item("L-440")).await;
    let spoil = |sql: &'static str, id: RunId| world.spoil(sql, id);
    // A caller of a kind this version does not know, a row that is gone,
    // and facts that are not facts at all.
    spoil(
        "UPDATE actions_run_calls SET caller = replace(caller, '\"person\"', '\"visitor\"') WHERE input_hash = ?1",
        unparsed_id,
    )
    .await;
    spoil(
        "DELETE FROM actions_run_calls WHERE input_hash = ?1",
        missing_id,
    )
    .await;
    spoil(
        "UPDATE actions_run_calls SET caller = '{}' WHERE input_hash = ?1",
        write_id,
    )
    .await;

    world.kernel.start_runner_loop(4).await.unwrap();
    for (run, id) in [
        (unparsed, unparsed_id),
        (missing, missing_id),
        (write, write_id),
    ] {
        let row = world
            .kernel
            .wait_for_rest(id, std::time::Duration::from_secs(30))
            .await
            .unwrap();
        assert_eq!(row.state, RunState::Failed, "{run}");
        let shown = world.call("run.get", json!({"run_id": run})).await;
        assert_eq!(
            code_of(&shown),
            (FailureClass::Busy, "run_call_unreadable".to_owned()),
            "{shown:?}"
        );
    }
    // Nothing reached the test application: no read with an empty input,
    // no write.
    assert_eq!(world.hooks.count("get_order"), 0);
    assert_eq!(world.hooks.count("get_item"), 0);
    assert!(world.app.writes().await.unwrap().is_empty());
    world.kernel.clone().close(CloseReason::Asked).await;
}

// ---- A4: the proof of presence -------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_desk_takes_a_proof_only_of_the_approver_and_of_now_and_records_what_it_says() {
    let world = World::ready().await;
    let desk = world.surface.desk();
    let (run, id) = world.rest_a_write("L-450").await;
    let waiting = desk.waiting(id).await.unwrap();
    let person = world.caller.principal.principal().clone();
    let now = world.now().as_unix_millis();
    let at = |millis: i64| Timestamp::from_unix_millis(millis);
    let refused = (FailureClass::Policy, "approval_refused".to_owned());

    // Another person's proof, one kept from an earlier prompt, and one from
    // a time that has not come.
    for (proof, why) in [
        (
            PresenceProof::new(
                PresenceCheck::OsPrompt,
                PrincipalId::from_bytes([9; 16]),
                at(now),
            ),
            "another person",
        ),
        (
            PresenceProof::new(
                PresenceCheck::OsPrompt,
                person.id,
                at(now - a2m_actions::PRESENCE_WINDOW_MS - 1),
            ),
            "older",
        ),
        (
            PresenceProof::new(PresenceCheck::OsPrompt, person.id, at(now + 60_000)),
            "ahead",
        ),
    ] {
        let failure = desk
            .approve(
                id,
                waiting.diff_hash,
                person.clone(),
                Channel::LocalPrompt,
                proof,
            )
            .await
            .unwrap_err();
        assert_eq!((failure.class(), failure.code().to_string()), refused);
        assert!(failure.message().contains(why), "{failure:?}");
    }
    // Nothing was approved: a resume still waits, and nothing was written.
    let waits = world.call("run.resume", json!({"run_id": run})).await;
    assert_eq!(status(&waits), "needs_approval", "{waits:?}");
    assert_eq!(world.app.write_count("A-102", "L-450").await.unwrap(), 0);

    // The approver's own proof, of now, naming the prompt that answered.
    let proof = PresenceProof::new(PresenceCheck::OsPrompt, person.id, at(now - 1_000))
        .naming("example_prompt");
    assert_eq!(proof.prompt(), Some("example_prompt"));
    // A name that is not a word is not kept.
    assert_eq!(proof.naming("Not a word!").prompt(), None);
    desk.approve(
        id,
        waiting.diff_hash,
        person.clone(),
        Channel::LocalPrompt,
        proof,
    )
    .await
    .unwrap();
    let done = world.call("run.resume", json!({"run_id": run})).await;
    assert_eq!(status(&done), "completed", "{done:?}");
    assert_eq!(world.app.write_count("A-102", "L-450").await.unwrap(), 1);

    // The audit log holds the kind of check, whose it was, when, and the
    // prompt, with the approval; and each refusal with its reason.
    let recorded = world.audited("approval.recorded").await;
    assert_eq!(recorded.len(), 1, "{recorded:?}");
    for part in [
        "presence=os_prompt".to_owned(),
        format!("of={}", person.id),
        format!("at={}", now - 1_000),
        "prompt=example_prompt".to_owned(),
        "channel=local_prompt".to_owned(),
    ] {
        assert!(recorded[0].contains(&part), "{part}: {recorded:?}");
    }
    assert_eq!(world.audited("approval.refused").await.len(), 3);
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_dispatcher_takes_a_proof_of_presence_only_of_its_caller_and_of_now() {
    let world = World::ready().await;
    let person = world.person();
    let set = world
        .call_as(
            &person,
            "killswitch.set",
            json!({"scope": "everything", "on": true}),
        )
        .await;
    assert!(set.ok, "{set:?}");
    let release = || {
        world.call_as(
            &person,
            "killswitch.set",
            json!({"scope": "everything", "on": false}),
        )
    };
    let refused = (FailureClass::Policy, "presence_required".to_owned());

    // A surface that hands back a proof it kept from ten minutes ago.
    *world.surface.proof_age_ms.lock().unwrap() = 600_000;
    let stale = release().await;
    assert_eq!(code_of(&stale), refused, "{stale:?}");
    assert_eq!(stale.exit, ExitClass::PersonNeeded);
    *world.surface.proof_age_ms.lock().unwrap() = 0;
    // A surface that hands back another person's proof.
    *world.surface.proof_of.lock().unwrap() = Some(PrincipalId::from_bytes([9; 16]));
    assert_eq!(code_of(&release().await), refused);
    *world.surface.proof_of.lock().unwrap() = None;
    // The switch is still set: work is refused.
    let held = world
        .call(
            "tool.call",
            json!({"tool": ORDER_GET, "input": {"order_id": "A-100"}}),
        )
        .await;
    assert_eq!(code_of(&held).1, "kill_switch_on", "{held:?}");

    // The caller's own proof, of now, releases it.
    let released = release().await;
    assert!(released.ok, "{released:?}");
    let read = world
        .call(
            "tool.call",
            json!({"tool": ORDER_GET, "input": {"order_id": "A-100"}}),
        )
        .await;
    assert!(read.ok, "{read:?}");
    // Setting and releasing are in the audit log, each written with the
    // switch itself.
    assert_eq!(world.audited("killswitch.set").await, ["on", "released"]);
    world.kernel.close(CloseReason::Asked).await;
}

// ---- A6: a person's act when it loosens ----------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_act_that_is_a_persons_when_it_loosens_is_asked_for_by_the_dispatcher() {
    let world = World::ready().await;
    let set = json!({"key": "logging.level", "value": "debug"});
    // `config.set` has no handler yet, so nothing in a handler asks. The
    // dispatcher does, before any handler: an agent is refused as no
    // person, and a person is asked for presence and only then told that
    // the action is not built.
    let by_agent = world.call("config.set", set.clone()).await;
    assert_eq!(
        code_of(&by_agent),
        (FailureClass::Policy, "person_required".to_owned()),
        "{by_agent:?}"
    );
    assert_eq!(world.surface.presence_asked.load(Ordering::SeqCst), 0);
    let by_person = world.call_as(&world.person(), "config.set", set).await;
    assert_eq!(code_of(&by_person).1, "action_not_built", "{by_person:?}");
    assert_eq!(world.surface.presence_asked.load(Ordering::SeqCst), 1);

    // Setting a kill switch tightens, and an agent may: nobody is asked.
    // Releasing one loosens.
    let on = world
        .call("killswitch.set", json!({"scope": "everything", "on": true}))
        .await;
    assert!(on.ok, "{on:?}");
    assert_eq!(world.surface.presence_asked.load(Ordering::SeqCst), 1);
    let off = world
        .call(
            "killswitch.set",
            json!({"scope": "everything", "on": false}),
        )
        .await;
    assert_eq!(code_of(&off).1, "person_required", "{off:?}");
    world.kernel.close(CloseReason::Asked).await;
}

// ---- A2: the protected settings and what is in force ---------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_protected_setting_that_is_not_in_force_stops_the_kernel_and_is_never_shown_as_in_force()
{
    // Every policy setting is carried into the policy engine (decision
    // 0145); what remains is an approver the policy engine has not.
    for (text, key) in [(
        "[policy]\napprove_writes = \"named_approver\"\n",
        "policy.approve_writes",
    )] {
        let world = world(Options::with_user_settings(text)).await;
        let health = world.kernel.health();
        assert_eq!(health.state, HealthState::Stopped, "{key}");
        assert!(
            health.reasons.iter().any(|reason| {
                reason.failure.code().as_str() == "setting_not_in_force"
                    && reason.failure.message().contains(key)
            }),
            "{key}: {health:?}"
        );
        // Nothing is served under a limit that is reported and not kept.
        let call = world.call("tool.search", json!({})).await;
        assert_eq!(
            code_of(&call),
            (FailureClass::Busy, "engine_degraded".to_owned())
        );
        // What the engine says of itself names the setting, and
        // `config.show` marks it as not in force.
        let status = world.ok("engine.status", json!({})).await;
        assert!(status.to_string().contains(key), "{status}");
        let doctor = world.ok("doctor.run", json!({})).await;
        let policy = doctor["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|check| check["check"] == "policy")
            .unwrap();
        assert_eq!(policy["state"], "config_drift", "{policy}");
        let shown = world
            .kernel
            .dispatch(Request::new(
                "config.show",
                json!({"key": key}),
                world.person(),
            ))
            .await;
        assert!(shown.ok, "{shown:?}");
        assert_eq!(
            shown.result.as_ref().unwrap()["settings"][0]["in_force"],
            false
        );
        assert!(
            shown.warnings.iter().any(|warning| warning.contains(key)),
            "{shown:?}"
        );
        world.kernel.close(CloseReason::Asked).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_limits_the_settings_show_are_the_limits_the_policy_engine_keeps() {
    // A tighter record limit from the settings, and the presets that set
    // policy settings, run, and the limit is kept (decision 0145).
    for text in [
        "[policy]\nmax_records_per_run = 5\n",
        "[profile]\npreset = \"cautious\"\n",
        "[profile]\npreset = \"ci\"\n",
    ] {
        let world = world(Options::with_user_settings(text)).await;
        assert_ne!(world.kernel.health().state, HealthState::Stopped, "{text}");
        let limit = u32::try_from(
            world
                .kernel
                .settings()
                .int("policy.max_records_per_run")
                .unwrap(),
        )
        .unwrap();
        assert!(world.within_limits(limit).await.is_ok(), "{text}");
        assert!(world.within_limits(limit + 1).await.is_err(), "{text}");
        world.kernel.close(CloseReason::Asked).await;
    }
    let world = World::ready().await;
    let settings = world.kernel.settings().clone();
    let limit = u32::try_from(settings.int("policy.max_records_per_run").unwrap()).unwrap();
    // The record limit shown is the one a run is refused past.
    assert!(world.within_limits(limit).await.is_ok());
    assert_eq!(
        world
            .within_limits(limit + 1)
            .await
            .unwrap_err()
            .code()
            .as_str(),
        "limit_exceeded"
    );
    // And nothing is marked as not in force.
    let shown = world
        .kernel
        .dispatch(Request::new("config.show", json!({}), world.person()))
        .await;
    assert!(shown.ok && shown.warnings.is_empty(), "{shown:?}");
    assert!(!shown.result.unwrap().to_string().contains("in_force"));
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pre_approved_write_needs_no_person_only_where_the_setting_allows_it() {
    let rule = |layers: &mut a2m_policy::PolicyLayers| {
        layers.pre_approvals.push(PreApproval {
            name: "small_writes".to_owned(),
            who: Subject::Anyone,
            scope: Scope::Pattern("*".to_owned()),
            max_records: 5,
            condition: None,
        });
    };
    let write = json!({"tool": ITEM_UPSERT, "input": item("L-460")});

    // The setting as it is by default: the rule of the policy set lets the
    // write through, and no person is asked.
    let allowed = World::ready().await;
    allowed.publish(rule).await;
    let done = allowed.call("tool.call", write.clone()).await;
    assert_eq!(status(&done), "completed", "{done:?}");
    assert_eq!(allowed.app.write_count("A-102", "L-460").await.unwrap(), 1);
    assert!(allowed.surface.approvals.lock().unwrap().is_empty());
    allowed.kernel.close(CloseReason::Asked).await;

    // The setting off, the same rule in the policy set: the write waits
    // for a person, nothing is written, and the run's event says why.
    let off = World::ready_with(Options::with_user_settings(
        "[policy]\npre_approval = false\n",
    ))
    .await;
    off.publish(rule).await;
    let rested = off.call("tool.call", write).await;
    assert_eq!(status(&rested), "needs_approval", "{rested:?}");
    assert_eq!(rested.exit, ExitClass::PersonNeeded);
    assert_eq!(off.app.write_count("A-102", "L-460").await.unwrap(), 0);
    let run = rested.result.unwrap()["run_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let trace = off
        .call_as(&off.person(), "trace.show", json!({"run_id": run}))
        .await;
    assert!(
        trace
            .result
            .unwrap()
            .to_string()
            .contains("policy.pre_approval"),
        "the need event names the setting"
    );
    // A person's approval carries it through, as for any write.
    let id: RunId = run.parse().unwrap();
    off.approve(id).await;
    let done = off.call("run.resume", json!({"run_id": run})).await;
    assert_eq!(status(&done), "completed", "{done:?}");
    assert_eq!(off.app.write_count("A-102", "L-460").await.unwrap(), 1);
    off.kernel.close(CloseReason::Asked).await;
}

// ---- A8: a decider that cannot decide ------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_write_the_policy_engine_cannot_decide_is_stopped_and_no_approval_carries_it() {
    let world = World::ready().await;
    // A pre-approval rule whose condition fails when it is run: the policy
    // engine cannot answer whether the write may proceed without a person.
    world
        .publish(|layers| {
            layers.pre_approvals.push(PreApproval {
                name: "breaks".to_owned(),
                who: Subject::Anyone,
                scope: Scope::Pattern("*".to_owned()),
                max_records: 5,
                condition: Some("1 / (records - records) == 1".to_owned()),
            });
        })
        .await;
    let stopped = world
        .call(
            "tool.call",
            json!({"tool": ITEM_UPSERT, "input": item("L-470")}),
        )
        .await;
    assert_eq!(
        code_of(&stopped),
        (FailureClass::Policy, "policy_evaluation_failed".to_owned()),
        "{stopped:?}"
    );
    // It does not wait for a person, so no approval can clear it.
    assert_ne!(status(&stopped), "needs_approval", "{stopped:?}");
    assert!(world.surface.approvals.lock().unwrap().is_empty());
    assert_eq!(world.app.write_count("A-102", "L-470").await.unwrap(), 0);
    world.kernel.close(CloseReason::Asked).await;
}

// ---- A7: the tool that was decided is the tool that is run ----------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tool_that_is_not_the_one_the_decision_was_made_for_is_not_run() {
    let world = World::ready().await;
    // A call as the dispatcher would hand it on had the tool been a read
    // when the decision was made, and been published as a write since.
    let decision = world.call("tool.search", json!({})).await.decision.unwrap();
    let input = json!({"tool": ITEM_UPSERT, "input": item("L-480"), "wait_seconds": 0});
    let call = crate::dispatch::Call {
        input: input.as_object().unwrap().clone(),
        caller: world.caller.clone(),
        decision,
        idempotency_key: None,
        project: None,
        project_refused: None,
        presence: None,
        decided: Some((a2m_core::EffectClass::Read, a2m_core::Hazard::None)),
    };
    let handler = crate::handlers::handler_for("tool.call").unwrap();
    let refused = handler(&world.kernel, call).await.unwrap_err();
    assert_eq!(
        (refused.class(), refused.code().as_str()),
        (FailureClass::Busy, "tool_changed")
    );
    // No run was made of it.
    assert_eq!(world.kernel.runs_in_hand().await.unwrap().working, 0);
    assert!(world.app.writes().await.unwrap().is_empty());
    // A debug print of a call names its fields and shows none of its values.
    let secret = Request::new(
        "secret.set",
        json!({"name": "example/key", "value": "example-key-shown-nowhere"}),
        world.caller.clone(),
    );
    let printed = format!("{secret:?}");
    assert!(
        printed.contains("value") && !printed.contains("example-key-shown-nowhere"),
        "{printed}"
    );
    world.kernel.close(CloseReason::Asked).await;
}

// ---- A14: the caller of a stopped engine ---------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_caller_a_stopped_engine_answers_to_is_nobody_and_holds_nothing() {
    let world = World::ready().await;
    // The store is open and the local user is known: the caller made
    // without a credential is still nobody, and is refused everything.
    let nobody = Caller::new(world.kernel.status_only_caller(), FrontDoor::Command, false);
    assert_ne!(nobody.principal, world.caller.principal);
    for (action, input) in [
        ("tool.search", json!({})),
        (
            "tool.call",
            json!({"tool": ORDER_GET, "input": {"order_id": "A-100"}}),
        ),
        ("audit.query", json!({})),
        ("killswitch.set", json!({"scope": "everything", "on": true})),
    ] {
        let refused = world.call_as(&nobody, action, input).await;
        assert_eq!(
            code_of(&refused),
            (FailureClass::Policy, "not_granted".to_owned()),
            "{action}"
        );
    }
    world.kernel.close(CloseReason::Asked).await;
}

// ---- What a write holds alone, and how long a lease lasts ----------------

/// The events of a run that say a job of it was handed to a runner.
async fn leases_of(world: &World, run: RunId) -> usize {
    world
        .kernel
        .events(&world.caller.principal, run, 0, 200)
        .await
        .unwrap()
        .events
        .iter()
        .filter(|event| event.kind == "lease_granted")
        .count()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_writes_on_one_surface_are_one_at_a_time_by_the_scheduler() {
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Condvar, Mutex};

    let world = World::ready().await;
    // The first step that reaches the driver stays there until the test
    // lets it go: its job is in a runner's hands, holding what it holds.
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let (entered, was_entered) = std::sync::mpsc::channel::<()>();
    let entered = Mutex::new(entered);
    let first = AtomicBool::new(true);
    let held = Arc::clone(&gate);
    world.hooks.on_step(move |_kind| {
        if first.swap(false, Ordering::SeqCst) {
            let _ = entered.lock().unwrap().send(());
            let (open, opened) = &*held;
            let mut open = open.lock().unwrap();
            while !*open {
                open = opened.wait(open).unwrap();
            }
        }
    });
    let start = |code: &str| {
        world.call(
            "tool.call",
            json!({"tool": ITEM_UPSERT, "input": item(code), "wait_seconds": 0}),
        )
    };
    let id_of = |response: &a2m_actions::Response| -> RunId {
        response.result.as_ref().unwrap()["run_id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap()
    };
    let first_run = id_of(&start("L-490").await);
    tokio::task::spawn_blocking(move || {
        was_entered
            .recv_timeout(std::time::Duration::from_secs(30))
            .unwrap();
    })
    .await
    .unwrap();

    // A second write on the same surface, while four slots are free and
    // only one is taken. The scheduler does not hand it out: the first
    // job holds the surface alone. Waiting here cannot make the test fail
    // by being slow; a scheduler that handed the job out would.
    let second_run = id_of(&start("L-491").await);
    let seen = world
        .kernel
        .events(&world.caller.principal, second_run, 0, 200)
        .await
        .unwrap()
        .last;
    let more = world
        .kernel
        .next_events(
            &world.caller.principal,
            second_run,
            seen,
            200,
            std::time::Duration::from_millis(1_500),
        )
        .await
        .unwrap();
    assert!(more.events.is_empty(), "{more:?}");
    assert_eq!(leases_of(&world, first_run).await, 1);
    assert_eq!(leases_of(&world, second_run).await, 0);
    let job = world
        .kernel
        .store()
        .unwrap()
        .read(move |tx| a2m_sched::Scheduler::unfinished_job(tx, second_run))
        .await
        .unwrap()
        .expect("the second run's job waits");
    let waiting = world.kernel.sched().unwrap().job(job).await.unwrap();
    assert_eq!(waiting.attempts, 0, "{waiting:?}");
    // The first job ends, at the rest for its approval, and the second is
    // handed out and runs to its own.
    {
        let (open, opened) = &*gate;
        *open.lock().unwrap() = true;
        opened.notify_all();
    }
    for run in [first_run, second_run] {
        let row = world
            .kernel
            .wait_for_rest(run, std::time::Duration::from_secs(30))
            .await
            .unwrap();
        assert_eq!(row.state, RunState::NeedsApproval, "{run}");
    }
    assert_eq!(leases_of(&world, second_run).await, 1);
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_write_whose_binding_has_six_steps_at_the_default_limit_fits_the_lease() {
    let world = World::ready().await;
    // Every step of the write's binding takes the engine's default limit,
    // thirty seconds, and there are six of them: the save, and the
    // read-back five times over. The engine sends the save only when the
    // lease leaves time for all of them.
    let mut plan = super::kernel::engine::plan(&world.app.url(""));
    plan.header.version = a2m_core::Version::new(2, 0, 0);
    let binding = &mut plan.operations[2].bindings[0];
    let read = binding.read_back[0].clone();
    binding.read_back.extend(std::iter::repeat_n(read, 4));
    for step in binding.act.iter_mut().chain(&mut binding.read_back) {
        step.timeout_ms = 0;
    }
    assert_eq!(binding.act.len() + binding.read_back.len(), 6);
    world.kernel.publish_plan(&plan).await.unwrap();

    let (run, id) = world.rest_a_write("L-495").await;
    world.approve(id).await;
    let done = world.call("run.resume", json!({"run_id": run})).await;
    assert_eq!(status(&done), "completed", "{done:?}");
    assert_eq!(world.app.write_count("A-102", "L-495").await.unwrap(), 1);
    world.kernel.close(CloseReason::Asked).await;
}

// ---- What the doors asked for (decision 0151) -----------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_follower_is_woken_when_its_run_comes_to_rest_and_not_when_it_already_rests() {
    let world = World::ready().await;
    let started = world
        .call(
            "tool.call",
            json!({"tool": ITEM_UPSERT, "input": item("L-500"), "wait_seconds": 0}),
        )
        .await;
    let id: RunId = started.result.unwrap()["run_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    // A follower reads on until the run is settled. A job's end appends no
    // event, so the last wait is ended by the run coming to rest alone:
    // long before the wait it was given runs out.
    let wait = std::time::Duration::from_secs(120);
    let began = std::time::Instant::now();
    let mut seen = a2m_actions::Followed::default();
    let last = loop {
        let page = world
            .kernel
            .next_events_from(&world.caller.principal, id, seen, 200, wait)
            .await
            .unwrap();
        seen = page.followed();
        if page.settled() {
            break page;
        }
    };
    assert_eq!(last.state, RunState::NeedsApproval);
    assert!(began.elapsed() < wait / 2, "{:?}", began.elapsed());
    let after = last.last;

    // The race between two calls of one follower: its last page showed the
    // run worked on, and the job ended before it called again. No event
    // says so; what the follower saw does, and it is told at once.
    let began = std::time::Instant::now();
    let told = world
        .kernel
        .next_events_from(
            &world.caller.principal,
            id,
            a2m_actions::Followed {
                after,
                settled: Some(false),
            },
            200,
            wait,
        )
        .await
        .unwrap();
    assert!(told.events.is_empty() && told.settled(), "{told:?}");
    assert!(began.elapsed() < wait / 2, "{:?}", began.elapsed());

    // A follower that begins on a run that already rests is not handed the
    // same empty page at once, again and again: it waits its wait out.
    let short = std::time::Duration::from_millis(300);
    let began = std::time::Instant::now();
    let resting = world
        .kernel
        .next_events(&world.caller.principal, id, after, 200, short)
        .await
        .unwrap();
    assert!(resting.events.is_empty() && resting.settled());
    assert!(began.elapsed() >= short, "{:?}", began.elapsed());

    // A second person of the workspace, as a test of a door makes one, can
    // follow nothing of it.
    let second = world.kernel.second_person([0x77; 16]).unwrap();
    assert_eq!(second.workspace(), world.caller.principal.workspace());
    assert!(world.kernel.events(&second, id, 0, 10).await.is_err());
    world.kernel.close(CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tools_are_listed_by_kind_and_a_next_step_names_a_tool_the_toolset_lists() {
    use a2m_actions::{BuiltInToolset, McpResultFace};

    let world = World::ready().await;
    let all = world.ok("tool.search", json!({"limit": 200})).await;
    let kinds = |listed: &Json| -> Vec<String> {
        listed["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["kind"].as_str().unwrap().to_owned())
            .collect()
    };
    for kind in ["operation", "pipeline"] {
        let some = world
            .ok("tool.search", json!({"kind": kind, "limit": 200}))
            .await;
        let found = kinds(&some);
        assert!(
            !found.is_empty() && found.iter().all(|one| one == kind),
            "{some}"
        );
        assert_eq!(
            found.len(),
            kinds(&all).iter().filter(|one| *one == kind).count()
        );
    }
    let neither = world.call("tool.search", json!({"kind": "screen"})).await;
    assert_eq!(code_of(&neither).1, "schema_invalid");

    // A run that ended badly points at `trace.show`, which the `use`
    // toolset does not list: for a caller on it the step is the same one
    // through `action.run`. A toolset that lists the action is given its
    // own tool.
    let (run, id) = world.rest_a_write("L-510").await;
    world
        .surface
        .desk()
        .decline(id, None, Channel::LocalPrompt)
        .await
        .unwrap();
    let mut ended = world.call("run.get", json!({"run_id": run})).await;
    ended.error = None;
    ended.next = Some(crate::dispatch::Next::new(
        "trace.show",
        json!({"run_id": run}),
        "to see what happened",
    ));
    let registry = world.kernel.actions();
    let on_use = McpResultFace::of_in(&ended, registry, BuiltInToolset::Use)
        .structured
        .unwrap();
    assert_eq!(on_use["next"]["tool"], "a2m_action_run", "{on_use}");
    assert_eq!(on_use["next"]["args"]["action"], "trace.show");
    assert_eq!(on_use["next"]["args"]["input"]["run_id"], run.as_str());
    ended.next = Some(crate::dispatch::Next::new(
        "run.resume",
        json!({"run_id": run}),
        "now",
    ));
    let listed = McpResultFace::of_in(&ended, registry, BuiltInToolset::Use)
        .structured
        .unwrap();
    assert_eq!(listed["next"]["tool"], "a2m_run_resume", "{listed}");
    world.kernel.close(CloseReason::Asked).await;
}
