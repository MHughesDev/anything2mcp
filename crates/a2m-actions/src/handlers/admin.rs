//! Administration: why a call was decided as it was, the audit log, a
//! stored secret, a kill switch, and the caller's own sessions.

use a2m_core::{CredentialRef, DecisionId, Failure, Identity, Secret, TargetName};
use a2m_ledger::{ChainCheck, Durability};
use a2m_policy::{KillScope, KillSwitch};
use a2m_sched::HoldScope;
use serde_json::{Value as Json, json};

use super::handler;
use crate::codes;
use crate::dispatch::{Next, Output};

/// Whether the caller holds the role of auditor or of administrator, asked
/// of the policy engine as the dispatcher asks about a role, and recorded
/// nowhere.
async fn may_explain(
    kernel: &crate::kernel::Kernel,
    call: &crate::dispatch::Call,
    policy: &a2m_policy::Policy,
) -> Result<(), Failure> {
    use a2m_policy::{ActionRef, Answer, Context, Question, Request, Resource, Role};
    let name = a2m_policy::ActionName::new("policy.explain")?;
    let (policy, principal) = (policy.clone(), call.caller.principal.principal.clone());
    let context = Context::plain(call.caller.principal.workspace, kernel.environment());
    let allowed = kernel
        .store()?
        .read(move |tx| {
            Ok([Role::Auditor, Role::Admin].into_iter().any(|needs| {
                let request = Request {
                    principal: principal.clone(),
                    question: Question::PerformAction,
                    resource: Resource::Action(ActionRef {
                        name: name.clone(),
                        needs,
                    }),
                    context: context.clone(),
                    run: None,
                    diff_hash: None,
                };
                policy.would(tx, &request) == Answer::Allow
            }))
        })
        .await?;
    if allowed {
        Ok(())
    } else {
        Err(codes::policy(
            a2m_policy::NOT_GRANTED,
            "Policy did not allow this action.",
            "Ask an administrator for the auditor or administrator role.",
        ))
    }
}

handler!(
    /// `policy.explain`: the recorded decision, the policy version, and the
    /// rules that made it.
    policy_explain,
    |kernel, call| {
        let id = call
            .required("decision")?
            .parse::<DecisionId>()
            .map_err(|_| {
                codes::input(
                    codes::SCHEMA_INVALID,
                    "That is not a decision's identifier.",
                    "Pass the `decision` a response carried.",
                )
            })?;
        let policy = kernel.policy().cloned().ok_or_else(|| {
            codes::busy(
                codes::ENGINE_DEGRADED,
                "The policy engine is not open.",
                "Call `doctor.run` for the fix.",
            )
        })?;
        // Only an auditor or an administrator may ask why. The registry lets
        // any viewer reach this far, and the policy engine says who holds
        // which role.
        may_explain(kernel, &call, &policy).await?;
        // A decision of another workspace is not told apart from one that
        // does not exist.
        let workspace = call.caller.principal.workspace;
        let explained = kernel
            .store()?
            .read(move |tx| policy.explain(tx, id, workspace))
            .await?;
        let mut out = json!({
            "decision": explained.id.to_string(),
            "answer": explained.answer.as_str(),
            "question": explained.question,
            "resource": explained.resource,
            "principal": explained.principal,
            "summary": explained.summary,
            "rules": explained.rules.iter().map(|rule| json!({
                "id": rule.id,
                "says": rule.says,
            })).collect::<Vec<_>>(),
        });
        if let Json::Object(map) = &mut out {
            if let Some(version) = explained.version {
                map.insert("version".to_owned(), json!(version));
            }
            if let Some(code) = explained.refusal_code {
                map.insert("refusal_code".to_owned(), json!(code));
            }
        }
        Ok(Output::ok(out))
    }
);

handler!(
    /// `audit.query`: the audit log, oldest first, and whether its chain is
    /// whole.
    audit_query,
    |kernel, call| {
        let workspace = call.caller.principal.workspace;
        let offset = u64::try_from(call.int("offset").unwrap_or(0)).unwrap_or(0);
        let limit = u32::try_from(call.int("limit").unwrap_or(100)).unwrap_or(100);
        let (rows, chain) = kernel
            .store()?
            .read(move |tx| {
                let rows = tx.audit().entries(&workspace, offset, limit)?;
                let chain = tx.audit().verify(&workspace)?;
                Ok((rows, chain))
            })
            .await?;
        let (chain, total) = match chain {
            ChainCheck::Intact { entries, .. } => ("intact", entries),
            ChainCheck::Broken { .. } => ("broken", 0),
        };
        let entries: Vec<Json> = rows
            .iter()
            .map(|row| {
                json!({
                    "seq": row.seq,
                    "at": row.at.as_unix_millis(),
                    "actor": row.actor,
                    "action": row.action,
                    "subject": row.subject,
                    "detail": row.detail,
                })
            })
            .collect();
        let complete = chain == "intact" && offset + entries.len() as u64 >= total;
        Ok(Output::ok(
            json!({"entries": entries, "chain": chain, "complete": complete}),
        ))
    }
);

handler!(
    /// `secret.set`: seals a secret under a name for the caller. The value
    /// is registered with the redactor before anything else and is never
    /// read back by any action.
    secret_set,
    |kernel, call| {
        let mut call = call;
        let name = call.required("name")?.to_owned();
        // Standard input is the command line's to read: it sends what it
        // read as `value` and never sends the flag. A call that still
        // carries the flag came from a face with no standard input, or from
        // a command line that did not read it.
        let from_stdin = call.flag("stdin").unwrap_or(false);
        // The value is taken out of the call, so that nothing that prints
        // or keeps the call afterwards holds it, and the bytes it was
        // handed over in are overwritten once the secret holds its own.
        let secret = match (call.input.remove("value"), from_stdin) {
            (Some(Json::String(value)), false) => {
                let secret = Secret::new(value.as_str());
                let mut handed = value.into_bytes();
                handed.fill(0);
                secret
            }
            (Some(_), true) => {
                return Err(codes::input(
                    codes::SCHEMA_INVALID,
                    "The call gives the secret both as `value` and from standard input.",
                    "Give one of them.",
                ));
            }
            (None, true) => {
                return Err(codes::input(
                    codes::SCHEMA_INVALID,
                    "The secret was to come from standard input, and the call carries none. Only the command line reads standard input.",
                    "Pipe the secret into `anything2mcp secret set <name> --stdin`.",
                ));
            }
            _ => {
                return Err(codes::input(
                    codes::SCHEMA_INVALID,
                    "`secret.set` needs the secret.",
                    "On a command line give `--stdin` and pipe the secret in; elsewhere give `value`.",
                ));
            }
        };
        let reference = CredentialRef::new(&format!("{}{name}", CredentialRef::SCHEME))
            .map_err(|_| {
                codes::input(
                    codes::SCHEMA_INVALID,
                    "The name of a secret is one to four parts joined by `/`, each of lowercase letters, digits, `_`, `-`, and `.`.",
                    "Choose a name such as `example/service/password`.",
                )
            })?;
        let credentials = kernel.inner.credentials.clone().ok_or_else(|| {
            codes::busy(
                codes::ENGINE_DEGRADED,
                "The vault is not open, so nothing can be sealed.",
                "Call `doctor.run` for the fix.",
            )
        })?;
        let principal = &call.caller.principal;
        credentials
            .put(
                &principal.workspace,
                &Identity::Person(principal.principal.id),
                &reference,
                &secret,
            )
            .await?;
        // The secret is sealed. A change that cannot be written to the
        // audit log is not reported as done.
        kernel
            .audit(
                &principal.principal.id.to_string(),
                "secret.set",
                reference.as_str(),
                "stored",
            )
            .await
            .map_err(|_| not_audited("The secret was stored"))?;
        Ok(Output::ok(
            json!({"name": reference.as_str(), "stored": true}),
        ))
    }
);

handler!(
    /// `killswitch.set`: stops, or releases, everything, the workspace, or
    /// one target. Policy refuses work it covers from the next decision,
    /// and the scheduler hands out no job it covers. Releasing one is a
    /// person's act.
    killswitch_set,
    |kernel, call| {
        let on = call.flag("on").unwrap_or(true);
        let workspace = call.caller.principal.workspace;
        let (scope, hold) = match call.required("scope")? {
            "workspace" => (
                KillScope::Workspace(workspace),
                HoldScope::Workspace(workspace),
            ),
            "target" => {
                let target = TargetName::new(call.text("target").ok_or_else(|| {
                    codes::input(
                        codes::SCHEMA_INVALID,
                        "The scope `target` needs the field `target`.",
                        "Give `target`.",
                    )
                })?)?;
                (
                    KillScope::Target(target.clone()),
                    HoldScope::Target(workspace, target),
                )
            }
            _ => (KillScope::Everything, HoldScope::Everything),
        };
        // Releasing a switch is a person's act. The dispatcher asked for
        // the proof before this ran, and checked that it is this caller's
        // and of now; a call that reached here without one releases
        // nothing.
        let presence = if on {
            None
        } else {
            Some(
                call.presence
                    .ok_or_else(|| {
                        codes::policy(
                            a2m_policy::PRESENCE_REQUIRED,
                            "Releasing a kill switch is a person's act, and the call carries no proof that one is present.",
                            "Run the command yourself, at a terminal, and answer the prompt.",
                        )
                    })?
                    .check(),
            )
        };
        let policy = kernel.policy().cloned().ok_or_else(|| {
            codes::busy(
                codes::ENGINE_DEGRADED,
                "The policy engine is not open.",
                "Call `doctor.run` for the fix.",
            )
        })?;
        let by = call.caller.principal.principal.id;
        let switch = KillSwitch {
            scope,
            on,
            by,
            presence,
        };
        let scope = call.required("scope")?.to_owned();
        let sched = kernel.sched()?;
        // The policy engine's switch and the scheduler's hold are two
        // writes. They are ordered so that a failure between them leaves
        // the stricter state, and one the policy engine explains: a switch
        // is set in the policy engine first, and released there last. The
        // audit entry is written in the policy engine's own transaction.
        if !on {
            sched.release_hold(hold.clone()).await?;
        }
        let entry = a2m_ledger::AuditEntry {
            workspace,
            actor: by.to_string(),
            action: "killswitch.set".to_owned(),
            subject: scope.clone(),
            detail: if on { "on" } else { "released" }.to_owned(),
        };
        let switched = kernel
            .store()?
            .write(Durability::Synced, move |tx| {
                let epoch = policy.set_kill_switch(tx, &switch)?;
                tx.audit().append(&entry)?;
                Ok(epoch)
            })
            .await;
        let epoch = match switched {
            Ok(epoch) => epoch,
            Err(failure) => {
                // The switch is still set in the policy engine: the hold
                // goes back, so that the queue and the decisions agree.
                if !on {
                    let _ = sched.hold(hold).await;
                }
                return Err(failure);
            }
        };
        if on {
            sched.hold(hold).await?;
        }
        Ok(
            Output::ok(json!({"scope": scope, "on": on, "epoch": epoch})).then(Next::new(
                "engine.status",
                json!({}),
                "to see the engine's state",
            )),
        )
    }
);

/// What a caller is told when a change was made and its audit entry could
/// not be written.
fn not_audited(done: &str) -> Failure {
    codes::busy(
        a2m_ledger::codes::STORE_UNAVAILABLE,
        format!("{done}, and the audit entry for it could not be written."),
        "Call `doctor.run`: the store may not be able to commit. Then do it again, so that the change is in the audit log.",
    )
}

handler!(
    /// `session.status`: the caller's own sessions. States and times,
    /// never a credential.
    session_status,
    |kernel, call| {
        let principal = &call.caller.principal;
        let identity = Identity::Person(principal.principal.id);
        let sessions = kernel
            .keeper()?
            .status(&principal.workspace, Some(&identity))
            .await?;
        let shown: Vec<Json> = sessions
            .iter()
            .filter(|session| {
                call.text("target")
                    .is_none_or(|target| target == session.target.as_str())
            })
            .map(|session| {
                json!({
                    "session": session.session.to_string(),
                    "target": session.target.as_str(),
                    "environment": session.environment.as_str(),
                    "provider": session.provider.as_str(),
                    "state": session.state.as_str(),
                    "wants": session.wants.map(a2m_auth::PersonReason::as_str),
                    "signed_in_at": session.signed_in_at.map(a2m_core::Timestamp::as_unix_millis),
                    "expires_at": session.expires_at.map(a2m_core::Timestamp::as_unix_millis),
                    "checked_at": session.checked_at.map(a2m_core::Timestamp::as_unix_millis),
                    "last_used_at": session.last_used_at.map(a2m_core::Timestamp::as_unix_millis),
                })
            })
            .collect();
        Ok(Output::ok(json!({"sessions": shown, "complete": true})))
    }
);
