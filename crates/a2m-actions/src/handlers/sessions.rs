//! Sessions: a sign-in an agent asks for and its person carries out, one of
//! the caller's own sessions ended, and a workspace's sessions listed and
//! ended by an administrator (decision 0205).
//!
//! Only the session keeper signs in. `session.login` finds the sign-in the
//! target's files declare for the surface, builds the keeper's entry from
//! the plan exactly as a run's lease does, and asks the keeper. A
//! credential is never an input: an agent's ask is shown to the person on a
//! page of the person surface, where the person types what the provider
//! needs; it reaches the handler beside the press, is sealed in the vault
//! for that person alone, and is never part of an input, a result, or a
//! log. A credential stored beforehand with `secret.set` is used as it is.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use a2m_auth::{AuthEntry, EndedBy, SessionKey, SessionStatus};
use a2m_core::{
    Environment, Failure, FailureClass, Identity, PrincipalId, SessionId, SurfaceIndex,
    SurfaceName, TargetName, WorkspaceId,
};
use serde_json::{Map, Value as Json, json};

use super::handler;
use crate::acts::AskedSecret;
use crate::codes;
use crate::dispatch::{Call, Caller, Next, Output};
use crate::kernel::Kernel;

/// The longest a sign-in is waited for within one call, whatever the
/// setting says: a press on a page waits for it.
const SIGN_IN_WAIT_LONGEST: Duration = Duration::from_secs(10 * 60);

/// The sign-in a target's files declare for one surface in one environment,
/// as the keeper is given it.
struct Declared {
    target: TargetName,
    surface: SurfaceName,
    environment: Environment,
    entry: AuthEntry,
}

/// The sign-in `input` names, or `None` when the target's files declare no
/// sign-in for the surface named, or for any surface when none is named.
/// The plan is the drafts' in a project that holds the target, and the
/// published version's otherwise, as the scout observes it.
async fn declared(
    kernel: &Kernel,
    project: Option<PathBuf>,
    input: &Map<String, Json>,
) -> Result<Option<Declared>, Failure> {
    let text = |name: &str| input.get(name).and_then(Json::as_str);
    let target = TargetName::new(text("target").unwrap_or_default())?;
    let (_, plan) = super::tools::plan_in(kernel, project, &target).await?;
    let index = match text("surface") {
        Some(named) => {
            let Some(at) = plan
                .surfaces
                .iter()
                .position(|surface| surface.name.as_str() == named)
            else {
                return Err(codes::input(
                    codes::NOT_FOUND,
                    format!(
                        "The target `{target}` declares no surface `{}`.",
                        codes::shown(named)
                    ),
                    "Name a surface the target file declares, or leave `surface` out.",
                ));
            };
            at
        }
        None => {
            let first = (0..plan.surfaces.len()).find(|at| {
                u32::try_from(*at)
                    .ok()
                    .and_then(|at| plan.auth_of(SurfaceIndex::new(at)))
                    .is_some()
            });
            let Some(at) = first else {
                return Ok(None);
            };
            at
        }
    };
    let Some(auth) = u32::try_from(index)
        .ok()
        .and_then(|at| plan.auth_of(SurfaceIndex::new(at)))
    else {
        return Ok(None);
    };
    let environment = match text("environment") {
        Some(named) => {
            let named = Environment::new(named)?;
            if !plan.environments.iter().any(|env| env.name == named) {
                return Err(codes::input(
                    codes::NOT_FOUND,
                    format!("The target `{target}` declares no environment `{named}`."),
                    "Name an environment the target file declares, or leave `environment` out.",
                ));
            }
            named
        }
        None => plan
            .default_environment()
            .map(|env| env.name.clone())
            .ok_or_else(|| {
                codes::input(
                    codes::NOT_FOUND,
                    format!("The target `{target}` declares no environment to sign in to."),
                    "Declare an environment in the target file, then ask again.",
                )
            })?,
    };
    Ok(Some(Declared {
        target,
        surface: plan.surfaces[index].name.clone(),
        environment,
        entry: crate::control::entry_of(auth),
    }))
}

/// The session's key: whose it is, on which target, in which environment.
/// A person's session is theirs alone.
fn key_of(workspace: WorkspaceId, person: PrincipalId, declared: &Declared) -> SessionKey {
    SessionKey {
        workspace,
        target: declared.target.clone(),
        environment: declared.environment.clone(),
        identity: Identity::Person(person),
    }
}

/// Each credential the entry names, with whether something is stored under
/// it for the person. Nothing is opened.
async fn roles(
    kernel: &Kernel,
    workspace: &WorkspaceId,
    person: PrincipalId,
    entry: &AuthEntry,
) -> Result<Vec<AskedSecret>, Failure> {
    let credentials = kernel.inner.credentials.clone().ok_or_else(|| {
        codes::busy(
            codes::ENGINE_DEGRADED,
            "The vault is not open, so no credential can be stored or used.",
            "Call `doctor.run` for the fix.",
        )
    })?;
    let mut out = Vec::new();
    for (role, reference) in &entry.credentials {
        let stored = credentials
            .holds(workspace, &Identity::Person(person), reference)
            .await?;
        out.push(AskedSecret {
            role: role.clone(),
            reference: reference.as_str().to_owned(),
            stored,
        });
    }
    Ok(out)
}

/// What a person's page shows for a sign-in, and the input it is carried
/// out with: the surface, the environment, and the provider pinned, the
/// lines the person reads, and the secrets the page asks for.
pub(crate) async fn pinned(
    kernel: &Kernel,
    caller: &Caller,
    mut input: Map<String, Json>,
    project: &(Option<PathBuf>, Option<Failure>),
) -> Result<(Map<String, Json>, Vec<String>, Vec<AskedSecret>), Failure> {
    let declared = declared(kernel, project.0.clone(), &input)
        .await?
        .ok_or_else(not_declared_failure)?;
    let person = caller.principal.principal.id;
    let secrets = roles(kernel, &caller.principal.workspace, person, &declared.entry).await?;
    input.insert("surface".to_owned(), json!(declared.surface.as_str()));
    input.insert(
        "environment".to_owned(),
        json!(declared.environment.as_str()),
    );
    let provider = declared.entry.provider.as_str().to_owned();
    let needs = kernel.keeper()?.needs(&declared.entry.provider)?;
    let mut details = vec![format!(
        "Signs in to the surface {} of {} in the environment {}, as you and for you alone, through the provider {provider}.",
        declared.surface, declared.target, declared.environment
    )];
    for secret in &secrets {
        details.push(if secret.stored {
            format!(
                "The {} is stored already under {}. Leave its field empty to use it, or type a new one to replace it.",
                secret.role, secret.reference
            )
        } else {
            format!(
                "Type the {} below. It is sealed in this machine's vault under {}, for you alone, and used for this target's sign-ins from then on. No program is shown it.",
                secret.role, secret.reference
            )
        });
    }
    if needs.person {
        details.push("The provider may then ask you for more, such as a one-time code: this page asks for it, and it is used for this one sign-in and kept nowhere.".to_owned());
    }
    if !needs.confirms {
        details.push("This provider holds the credential and does not try it: whether the system accepts it is known when it is first used.".to_owned());
    }
    Ok((input, details, secrets))
}

/// What a caller is told when a change was made and its audit entry could
/// not be written.
fn not_audited(done: &str) -> Failure {
    codes::busy(
        a2m_ledger::codes::STORE_UNAVAILABLE,
        format!("{done}, and the audit entry for it could not be written."),
        "Call `doctor.run`: the store may not be able to commit. Then do it again, so that the change is in the audit log.",
    )
}

fn not_declared_failure() -> Failure {
    codes::input(
        codes::NOT_FOUND,
        "The target's files declare no sign-in for that surface.",
        "Declare it in the target's `auth.yaml` with `draft.write`, naming the provider and a vault reference for each credential, then ask again.",
    )
}

/// The session as a caller is told of it: that it exists, for whom, for
/// which environment, until when, and how far it is known to hold. Never
/// its contents.
fn shown(status: &SessionStatus, surface: &SurfaceName, confirms: bool) -> Json {
    let person = match &status.identity {
        Identity::Person(person) => person.to_string(),
        other => other.to_string(),
    };
    json!({
        "state": if status.state == a2m_core::SessionState::SigningIn { "signing_in" } else { "signed_in" },
        "session": status.session.to_string(),
        "target": status.target.as_str(),
        "surface": surface.as_str(),
        "environment": status.environment.as_str(),
        "for": person,
        "provider": status.provider.as_str(),
        "session_state": status.state.as_str(),
        "signed_in_at": status.signed_in_at.map(a2m_core::Timestamp::as_unix_millis),
        "expires_at": status.expires_at.map(a2m_core::Timestamp::as_unix_millis),
        // `confirmed` when the provider's sign-in means the system accepted
        // the credential; `held` when the provider only holds it, and the
        // first use is the first test of it.
        "level": if confirms { "confirmed" } else { "held" },
    })
}

/// How long a sign-in is waited for within one call.
fn sign_in_wait(kernel: &Kernel) -> Duration {
    let minutes = kernel
        .settings()
        .int("sessions.login_wait_minutes")
        .unwrap_or(5);
    let seconds = u64::try_from(minutes).unwrap_or(0).saturating_mul(60);
    Duration::from_secs(seconds).min(SIGN_IN_WAIT_LONGEST)
}

handler!(
    /// `session.login`: a sign-in to one surface of a target, for the
    /// caller's person, by the session keeper. An agent asks; where a
    /// credential is not stored yet, or the provider needs a person, the
    /// ask is shown to the person on a page of the person surface, and is
    /// carried out only from there. A credential the person types reaches
    /// this handler beside the press and is sealed for them alone; the
    /// result says only that a session exists, for whom, where, until
    /// when, and how far it is known to hold.
    session_login,
    |kernel, call| {
        let principal = &call.caller.principal;
        let workspace = principal.workspace;
        let me = principal.principal.id;
        if call.caller.principal.person().is_none() {
            return Err(codes::policy(
                codes::PERSON_REQUIRED,
                "A sign-in is a person's, and this caller acts for no person.",
                "Ask the person to sign in, through a program that acts for them.",
            ));
        }
        let Some(declared) = declared(kernel, call.project.clone(), &call.input).await? else {
            let target = call.required("target")?;
            return Ok(Output::ok(json!({
                "state": "not_needed",
                "target": target,
                "why": "The target's files declare no sign-in for any surface, so there is nothing to sign in to. If a surface refuses reads for want of one, declare it in `auth.yaml` with `draft.write`, naming the provider and a vault reference for each credential, then ask again.",
            })));
        };
        let keeper = kernel.keeper()?;
        let needs = keeper.needs(&declared.entry.provider)?;
        let asked = roles(kernel, &workspace, me, &declared.entry).await?;
        let missing: Vec<&AskedSecret> = asked.iter().filter(|secret| !secret.stored).collect();

        let key = key_of(workspace, me, &declared);
        // An agent's ask that a person must answer goes to the person's
        // page; nothing is done until they carry it out there. A session
        // that holds already is said to, and nobody is asked.
        if call.caller.is_agent && call.presence.is_none() && (needs.person || !missing.is_empty())
        {
            let holding = keeper
                .status(&workspace, Some(&key.identity))
                .await?
                .into_iter()
                .find(|session| {
                    session.target == key.target
                        && session.environment == key.environment
                        && matches!(
                            session.state,
                            a2m_core::SessionState::Valid
                                | a2m_core::SessionState::Suspect
                                | a2m_core::SessionState::Renewing
                        )
                });
            if let Some(holding) = holding {
                return Ok(
                    Output::ok(shown(&holding, &declared.surface, needs.confirms)).then(Next::new(
                        "session.status",
                        json!({"target": declared.target.as_str()}),
                        "to see the session as it stands; a run of the target uses it",
                    )),
                );
            }
            let def = kernel
                .actions()
                .get(&"session.login".parse()?)
                .cloned()
                .ok_or_else(|| {
                    codes::input(
                        codes::ACTION_NOT_BUILT,
                        "`session.login` is not in this engine's registry.",
                        "Call `action.search` to see which actions are built.",
                    )
                })?;
            return kernel
                .ask_person(
                    &def,
                    &call.caller,
                    call.input.clone(),
                    (call.project.clone(), call.project_refused.clone()),
                )
                .await;
        }

        // What the person typed is sealed for them alone, under the
        // references the auth file names, as `secret.set` seals it.
        if let Some(given) = &call.given {
            let credentials = kernel.inner.credentials.clone().ok_or_else(|| {
                codes::busy(
                    codes::ENGINE_DEGRADED,
                    "The vault is not open, so nothing can be sealed.",
                    "Call `doctor.run` for the fix.",
                )
            })?;
            for (role, secret) in given.iter() {
                let Some(reference) = declared.entry.credentials.get(role) else {
                    continue;
                };
                credentials
                    .put(&workspace, &Identity::Person(me), reference, secret)
                    .await?;
                kernel
                    .audit(
                        &me.to_string(),
                        "session.login",
                        reference.as_str(),
                        "stored",
                    )
                    .await
                    .map_err(|_| not_audited("The credential was stored"))?;
            }
        }
        let still: Vec<&AskedSecret> = missing
            .iter()
            .copied()
            .filter(|secret| {
                call.given
                    .as_ref()
                    .is_none_or(|given| given.get(&secret.role).is_none())
            })
            .collect();
        if let Some(first) = still.first() {
            return Err(Failure::new(
                FailureClass::Input,
                a2m_auth::codes::CREDENTIAL_MISSING,
                format!(
                    "The sign-in needs the {} stored under `{}`, and nothing is stored there.",
                    first.role, first.reference
                ),
                "Type it on the sign-in page when an agent asks, or store it yourself with `anything2mcp secret set <name> --stdin`.",
            ));
        }

        let entry = Arc::new(declared.entry);
        let ticket = keeper
            .sign_in(&key, Arc::clone(&entry), call.caller.principal.person())
            .await?;
        let status = keeper
            .signed_in(ticket.session, sign_in_wait(kernel))
            .await
            .map_err(|failure| kernel.redact().redact_failure(&failure))?;
        kernel
            .audit(
                &me.to_string(),
                "session.login",
                &status.session.to_string(),
                status.state.as_str(),
            )
            .await
            .ok();
        let output = Output::ok(shown(&status, &declared.surface, needs.confirms));
        Ok(output.then(Next::new(
            "session.status",
            json!({"target": declared.target.as_str()}),
            "to see the session as it stands; a run of the target uses it",
        )))
    }
);

/// One of the sessions `status` holds, by its identifier.
fn find(sessions: Vec<SessionStatus>, id: SessionId) -> Result<SessionStatus, Failure> {
    sessions
        .into_iter()
        .find(|session| session.session == id)
        .ok_or_else(|| {
            codes::input(
                codes::NOT_FOUND,
                "No session of yours has that identifier.",
                "Call `session.status` for your sessions.",
            )
        })
}

fn session_id(call: &Call) -> Result<SessionId, Failure> {
    call.required("session_id")?
        .parse::<SessionId>()
        .map_err(|_| {
            codes::input(
                codes::SCHEMA_INVALID,
                "That is not a session's identifier.",
                "Pass the `session` a result of `session.status` gave.",
            )
        })
}

fn key_from(status: &SessionStatus, workspace: WorkspaceId) -> SessionKey {
    SessionKey {
        workspace,
        target: status.target.clone(),
        environment: status.environment.clone(),
        identity: status.identity.clone(),
    }
}

handler!(
    /// `session.revoke`: ends one of the caller's own sessions. The keeper
    /// marks it first, so no new lease is given, signs out where the
    /// provider can, and drops what was held. Another person's session is
    /// not told apart from one that does not exist.
    session_revoke,
    |kernel, call| {
        let principal = &call.caller.principal;
        let (workspace, me) = (principal.workspace, principal.principal.id);
        let id = session_id(&call)?;
        let keeper = kernel.keeper()?;
        let session = find(
            keeper
                .status(&workspace, Some(&Identity::Person(me)))
                .await?,
            id,
        )?;
        keeper
            .revoke(&key_from(&session, workspace), EndedBy::Owner)
            .await?;
        kernel
            .audit(
                &me.to_string(),
                "session.revoke",
                &id.to_string(),
                "revoked",
            )
            .await
            .map_err(|_| not_audited("The session was ended"))?;
        Ok(Output::ok(json!({
            "session": id.to_string(),
            "target": session.target.as_str(),
            "environment": session.environment.as_str(),
            "state": "revoked",
        })))
    }
);

/// A session as an administrator sees it: whose, where, and its state and
/// times. Never a credential, and nothing that would let anyone use it.
fn listed(status: &SessionStatus) -> Json {
    json!({
        "session": status.session.to_string(),
        "identity": status.identity.to_string(),
        "target": status.target.as_str(),
        "environment": status.environment.as_str(),
        "provider": status.provider.as_str(),
        "state": status.state.as_str(),
        "wants": status.wants.map(a2m_auth::PersonReason::as_str),
        "signed_in_at": status.signed_in_at.map(a2m_core::Timestamp::as_unix_millis),
        "expires_at": status.expires_at.map(a2m_core::Timestamp::as_unix_millis),
        "last_used_at": status.last_used_at.map(a2m_core::Timestamp::as_unix_millis),
    })
}

handler!(
    /// `session.list`: every session of the workspace, without contents.
    session_list,
    |kernel, call| {
        let workspace = call.caller.principal.workspace;
        let sessions = kernel.keeper()?.status(&workspace, None).await?;
        let shown: Vec<Json> = sessions
            .iter()
            .filter(|session| {
                call.text("target")
                    .is_none_or(|target| target == session.target.as_str())
            })
            .map(listed)
            .collect();
        Ok(Output::ok(json!({"sessions": shown, "complete": true})))
    }
);

handler!(
    /// `session.end`: an administrator ends someone's session. It is ended
    /// without being used: nothing of it is leased, read, or entered, and
    /// the person signs in again when they next need it.
    session_end,
    |kernel, call| {
        let principal = &call.caller.principal;
        let workspace = principal.workspace;
        let id = session_id(&call)?;
        let keeper = kernel.keeper()?;
        let session = find(keeper.status(&workspace, None).await?, id)?;
        keeper
            .revoke(&key_from(&session, workspace), EndedBy::Administrator)
            .await?;
        kernel
            .audit(
                &principal.principal.id.to_string(),
                "session.end",
                &id.to_string(),
                "ended",
            )
            .await
            .map_err(|_| not_audited("The session was ended"))?;
        Ok(Output::ok(json!({
            "session": id.to_string(),
            "identity": session.identity.to_string(),
            "target": session.target.as_str(),
            "state": "revoked",
        })))
    }
);
