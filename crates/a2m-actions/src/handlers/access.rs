//! Access: toolsets, grants, and policy sets, as actions over the policy
//! engine's layers (decision 0328, package 6.3).
//!
//! The policy engine decides and compiles; this module only turns what an
//! administrator says into a layer, a test, or a question, and hands it to
//! the engine. Every change is published as a new version by a person: the
//! presence check comes from the dispatcher (`PersonNeed::Presence`), and a
//! second administrator, where the set in force asks for one, proves
//! presence of their own through the person surface.

use std::path::{Component, Path};

use a2m_core::{Digest, EffectClass, Failure, OperationName, PrincipalId, TargetName};
use a2m_policy::{
    Answer, Context, Grant, GroupName, IdentityMode, LayerChange, ModuleName, PolicyLayers,
    PolicyTest, PrincipalKind, PrincipalRef, PublishProof, PublishReview, Question, Request,
    Resource, Scope, Subject, ToolRef, Toolset, ToolsetName,
};
use serde_json::{Value as Json, json};

use super::handler;
use crate::codes;
use crate::def::ActionName;
use crate::dispatch::{Call, Output};
use crate::kernel::Kernel;
use crate::person::PresenceAsk;

/// The longest policy file the engine reads.
const FILE_LONGEST: u64 = 1 << 20;

/// The most rows a test table may hold.
const TESTS_MOST: usize = 500;

fn invalid(what: impl Into<String>, next: impl Into<String>) -> Failure {
    codes::input(codes::SCHEMA_INVALID, what, next)
}

fn policy_of(kernel: &Kernel) -> Result<a2m_policy::Policy, Failure> {
    kernel.policy().cloned().ok_or_else(|| {
        codes::busy(
            codes::ENGINE_DEGRADED,
            "The policy engine is not open.",
            "Call `doctor.run` for the fix.",
        )
    })
}

// ---- Words in and out ----------------------------------------------------

/// Who, as an administrator writes it: `anyone`, `group:<name>`, or a
/// principal's identifier.
fn subject_of(text: &str) -> Result<Subject, Failure> {
    if text == "anyone" {
        return Ok(Subject::Anyone);
    }
    if let Some(group) = text.strip_prefix("group:") {
        return Ok(Subject::Group(GroupName::new(group)?));
    }
    text.parse::<PrincipalId>()
        .map(Subject::Principal)
        .map_err(|_| {
            invalid(
                "A subject is `anyone`, `group:<name>`, or a principal's identifier.",
                "Write who as one of those three.",
            )
        })
}

fn subject_text(subject: &Subject) -> String {
    match subject {
        Subject::Anyone => "anyone".to_owned(),
        Subject::Group(group) => format!("group:{}", group.as_str()),
        Subject::Principal(id) => id.to_string(),
    }
}

/// Which tools, as an administrator writes it: a tool's name or a pattern
/// with `*`, `target:<name>`, or `module:<target>/<module>`.
fn scope_of(text: &str) -> Result<Scope, Failure> {
    if let Some(target) = text.strip_prefix("target:") {
        return Ok(Scope::Target(TargetName::new(target)?));
    }
    if let Some(rest) = text.strip_prefix("module:") {
        let Some((target, module)) = rest.split_once('/') else {
            return Err(invalid(
                "A module scope is `module:<target>/<module>`.",
                "Write it in that form.",
            ));
        };
        return Ok(Scope::Module(
            TargetName::new(target)?,
            ModuleName::new(module)?,
        ));
    }
    if text.is_empty() || text.len() > 200 || text.chars().any(|c| c.is_whitespace()) {
        return Err(invalid(
            "A tool pattern is a tool's name, with `*` for any run of characters.",
            "Write a name or a pattern such as `example_order_*`.",
        ));
    }
    Ok(Scope::Pattern(text.to_owned()))
}

fn scope_text(scope: &Scope) -> String {
    match scope {
        Scope::Target(target) => format!("target:{}", target.as_str()),
        Scope::Module(target, module) => format!("module:{}/{}", target.as_str(), module.as_str()),
        Scope::Pattern(pattern) => pattern.clone(),
    }
}

fn effect_of(text: &str) -> Result<EffectClass, Failure> {
    text.parse::<EffectClass>().map_err(|_| {
        invalid(
            format!("`{text}` is not an effect class."),
            "Use `read`, `write`, or `irreversible`.",
        )
    })
}

fn mode_of(text: &str) -> Result<IdentityMode, Failure> {
    IdentityMode::from_text(text).ok_or_else(|| {
        invalid(
            format!("`{text}` is not an identity mode."),
            "Use `per_user_token`, `per_user_session`, `per_user_device`, or `service_account`.",
        )
    })
}

fn texts(call: &Call, field: &str) -> Result<Option<Vec<String>>, Failure> {
    let Some(value) = call.input.get(field) else {
        return Ok(None);
    };
    let Some(items) = value.as_array() else {
        return Err(invalid(
            format!("`{field}` is a list of texts."),
            "Give a list.",
        ));
    };
    items
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_owned)
                .ok_or_else(|| invalid(format!("`{field}` holds only texts."), "Give texts."))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

fn grant_json(grant: &Grant) -> Json {
    json!({
        "who": subject_text(&grant.who),
        "scope": scope_text(&grant.scope),
        "max_effect": grant.max_effect.to_string(),
        "identity_mode": grant.identity_mode.as_str(),
    })
}

fn toolset_json(toolset: &Toolset) -> Json {
    json!({
        "name": toolset.name.as_str(),
        "who": toolset.who.iter().map(subject_text).collect::<Vec<_>>(),
        "tools": toolset.tools.iter().map(scope_text).collect::<Vec<_>>(),
        "max_effect": toolset.max_effect.to_string(),
        "identity_mode": toolset.identity_mode.as_str(),
        "allow_pre_approval": toolset.allow_pre_approval,
    })
}

// ---- Test rows -----------------------------------------------------------

/// One row of a test table, as a map: `name`, `principal` (an identifier),
/// `agent` (true when the caller is an agent), `group`, `target`, `module`,
/// `tool`, `effect`, `toolset`, and `expect` (`allow` or `deny`). The
/// question is always whether the principal may use the tool.
fn test_of(row: &Json, kernel: &Kernel, call: &Call) -> Result<PolicyTest, Failure> {
    let bad = |why: &str| {
        invalid(
            format!("A row of the test table is not taken: {why}."),
            "Each row gives `name`, `principal`, `target`, `tool`, `effect`, and `expect`; it may give `agent`, `group`, `module`, and `toolset`.",
        )
    };
    let map = row.as_object().ok_or_else(|| bad("it is not a map"))?;
    let text = |name: &str| map.get(name).and_then(Json::as_str);
    let need = |name: &str| text(name).ok_or_else(|| bad(&format!("`{name}` is missing")));
    let id = need("principal")?
        .parse::<PrincipalId>()
        .map_err(|_| bad("`principal` is not an identifier"))?;
    let agent = map.get("agent").and_then(Json::as_bool).unwrap_or(false);
    let mut principal = if agent {
        PrincipalRef {
            id,
            kind: PrincipalKind::Agent,
            groups: Vec::new(),
        }
    } else {
        PrincipalRef::person(id)
    };
    if let Some(group) = text("group") {
        principal.groups.push(GroupName::new(group)?);
    }
    let tool = ToolRef {
        target: TargetName::new(need("target")?)?,
        module: ModuleName::new(text("module").unwrap_or("main"))?,
        tool: OperationName::new(need("tool")?)?,
    };
    let mut context = Context::plain(call.caller.principal.workspace, kernel.environment());
    context.effect = effect_of(need("effect")?)?;
    context.caller_is_agent = agent;
    if let Some(toolset) = text("toolset") {
        context.toolset = Some(ToolsetName::new(toolset)?);
    }
    let expect = match need("expect")? {
        "allow" => Answer::Allow,
        "deny" => Answer::Deny,
        _ => return Err(bad("`expect` is `allow` or `deny`")),
    };
    Ok(PolicyTest {
        name: need("name")?.to_owned(),
        request: Request {
            principal,
            question: Question::UseTool,
            resource: Resource::Tool(tool),
            context,
            run: None,
            diff_hash: None,
        },
        expect,
    })
}

fn tests_of(rows: &[Json], kernel: &Kernel, call: &Call) -> Result<Vec<PolicyTest>, Failure> {
    if rows.len() > TESTS_MOST {
        return Err(invalid(
            format!("A test table holds at most {TESTS_MOST} rows."),
            "Split the table.",
        ));
    }
    rows.iter().map(|row| test_of(row, kernel, call)).collect()
}

fn tests_input(kernel: &Kernel, call: &Call) -> Result<Vec<PolicyTest>, Failure> {
    match call.input.get("tests").and_then(Json::as_array) {
        Some(rows) => tests_of(rows, kernel, call),
        None => Ok(Vec::new()),
    }
}

// ---- The catalog of published tools ---------------------------------------

/// Every published tool with the context its own facts give, for the
/// questions asked over a catalog.
async fn catalog_of(
    kernel: &Kernel,
    call: &Call,
    toolset: Option<&ToolsetName>,
) -> Result<Vec<(ToolRef, Context)>, Failure> {
    let (compiled_targets, _) = kernel.current_compiled_targets().await?;
    let mut out = Vec::new();
    for (target, hash, compiled_target) in &compiled_targets {
        for tool in &compiled_target.tools {
            let found = crate::tools::FoundTool {
                target: target.clone(),
                compiled_target_hash: *hash,
                compiled_target: std::sync::Arc::clone(compiled_target),
                tool: tool.clone(),
            };
            let (effect, hazard) = found.effect();
            let mut context = Context::plain(call.caller.principal.workspace, kernel.environment());
            context.effect = effect;
            context.hazard = hazard;
            context.toolset = toolset.cloned();
            let module = ModuleName::new(tool.module.as_deref().unwrap_or("main"))?;
            out.push((
                ToolRef {
                    target: target.clone(),
                    module,
                    tool: tool.name.clone(),
                },
                context,
            ));
        }
    }
    Ok(out)
}

// ---- Publishing -----------------------------------------------------------

/// The second administrator, where the call names one: the person surface
/// asks that person for proof of presence of their own, and the proof must be
/// theirs and of now.
async fn review_of(
    kernel: &Kernel,
    call: &Call,
    action: &str,
) -> Result<Option<PublishReview>, Failure> {
    let Some(named) = call.text("reviewer") else {
        return Ok(None);
    };
    let reviewer = named.parse::<PrincipalId>().map_err(|_| {
        invalid(
            "`reviewer` is not a principal's identifier.",
            "Name the second administrator by identifier.",
        )
    })?;
    let drafted_by = call
        .text("drafted_by")
        .map(|text| {
            text.parse::<PrincipalId>().map_err(|_| {
                invalid(
                    "`drafted_by` is not a principal's identifier.",
                    "Name the author by identifier.",
                )
            })
        })
        .transpose()?;
    let proof = kernel
        .person()
        .confirm_presence(PresenceAsk {
            principal: reviewer,
            action: ActionName::new(action)?,
            effect: EffectClass::Write,
            summary: "Review and publish a change to the policy set.".to_owned(),
        })
        .await?;
    proof
        .checked(reviewer, kernel.inner.clock.now())
        .map_err(|why| {
            codes::policy(
                a2m_policy::PRESENCE_REQUIRED,
                format!("The second administrator's proof of presence was not taken: {why}."),
                "Ask the second administrator to answer the prompt themselves.",
            )
        })?;
    Ok(Some(PublishReview {
        reviewer: PrincipalRef::person(reviewer),
        drafted_by,
    }))
}

fn proof_of(call: &Call) -> PublishProof {
    PublishProof {
        presence: call
            .presence
            .as_ref()
            .map(crate::person::PresenceProof::check),
    }
}

fn published(version: Digest, what: &str) -> Output {
    Output::ok(json!({"version": version.to_string(), "changed": what}))
}

/// Publishes one change to the layers in force. `prepare` reads the layers
/// in the same transaction and says which change to make, or refuses.
async fn change<F>(
    kernel: &Kernel,
    call: &Call,
    action: &'static str,
    what: String,
    prepare: F,
) -> Result<Output, Failure>
where
    F: FnOnce(&PolicyLayers) -> Result<LayerChange, Failure> + Send + 'static,
{
    let policy = policy_of(kernel)?;
    let tests = tests_input(kernel, call)?;
    let review = review_of(kernel, call, action).await?;
    let by = call.caller.principal.principal.clone();
    let proof = proof_of(call);
    let version = kernel
        .store()?
        .write(a2m_ledger::Durability::Synced, move |tx| {
            let Some(layers) = policy.layers_in_force(tx) else {
                return Err(invalid(
                    "No valid policy set is in force to change.",
                    "Call `doctor.run` for the fix.",
                ));
            };
            let change = prepare(&layers)?;
            policy.publish_change(tx, change, tests, &by, proof, review.as_ref())
        })
        .await?;
    kernel
        .audit(
            &call.caller.principal.principal.id.to_string(),
            action,
            &what,
            &version.to_string(),
        )
        .await?;
    Ok(published(version, &what))
}

// ---- Toolsets ---------------------------------------------------------------

fn subjects_of(call: &Call, field: &str) -> Result<Option<Vec<Subject>>, Failure> {
    texts(call, field)?
        .map(|items| items.iter().map(|text| subject_of(text)).collect())
        .transpose()
}

fn scopes_of(call: &Call, field: &str) -> Result<Option<Vec<Scope>>, Failure> {
    texts(call, field)?
        .map(|items| items.iter().map(|text| scope_of(text)).collect())
        .transpose()
}

handler!(
    /// `toolset.create`: a new toolset. It only narrows what grants allow.
    toolset_create,
    |kernel, call| {
        let name = ToolsetName::new(call.required("name")?)?;
        let who = subjects_of(&call, "who")?.unwrap_or_default();
        let tools = scopes_of(&call, "tools")?.unwrap_or_default();
        let max_effect = effect_of(call.required("max_effect")?)?;
        let identity_mode = mode_of(call.text("identity_mode").unwrap_or("per_user_session"))?;
        let toolset = Toolset {
            name: name.clone(),
            who,
            tools,
            max_effect,
            identity_mode,
            allow_pre_approval: call.flag("writes_without_person").unwrap_or(false),
        };
        let named = name.clone();
        change(
            kernel,
            &call,
            "toolset.create",
            format!("toolset {name}"),
            move |layers| {
                if layers.toolsets.iter().any(|held| held.name == named) {
                    return Err(invalid(
                        format!("A toolset named `{named}` already exists."),
                        "Change it with `toolset.update`.",
                    ));
                }
                Ok(LayerChange::SetToolset(toolset))
            },
        )
        .await
    }
);

handler!(
    /// `toolset.update`: changes the fields of a toolset that are given.
    toolset_update,
    |kernel, call| {
        let name = ToolsetName::new(call.required("name")?)?;
        let who = subjects_of(&call, "who")?;
        let tools = scopes_of(&call, "tools")?;
        let max_effect = call.text("max_effect").map(effect_of).transpose()?;
        let identity_mode = call.text("identity_mode").map(mode_of).transpose()?;
        let allow = call.flag("writes_without_person");
        let named = name.clone();
        change(
            kernel,
            &call,
            "toolset.update",
            format!("toolset {name}"),
            move |layers| {
                let mut toolset = layers
                    .toolsets
                    .iter()
                    .find(|held| held.name == named)
                    .cloned()
                    .ok_or_else(|| {
                        codes::input(
                            codes::NOT_FOUND,
                            format!("No toolset is named `{named}`."),
                            "Call `toolset.list` to see the toolsets.",
                        )
                    })?;
                if let Some(who) = who {
                    toolset.who = who;
                }
                if let Some(tools) = tools {
                    toolset.tools = tools;
                }
                if let Some(effect) = max_effect {
                    toolset.max_effect = effect;
                }
                if let Some(mode) = identity_mode {
                    toolset.identity_mode = mode;
                }
                if let Some(allow) = allow {
                    toolset.allow_pre_approval = allow;
                }
                Ok(LayerChange::SetToolset(toolset))
            },
        )
        .await
    }
);

handler!(
    /// `toolset.remove`: removes a toolset by name.
    toolset_remove,
    |kernel, call| {
        let name = ToolsetName::new(call.required("name")?)?;
        let named = name.clone();
        change(
            kernel,
            &call,
            "toolset.remove",
            format!("toolset {name}"),
            move |_| Ok(LayerChange::RemoveToolset(named)),
        )
        .await
    }
);

handler!(
    /// `toolset.list`: the toolsets in force, each with the published tools
    /// it selects.
    toolset_list,
    |kernel, call| {
        let policy = policy_of(kernel)?;
        let catalog: Vec<ToolRef> = catalog_of(kernel, &call, None)
            .await?
            .into_iter()
            .map(|(tool, _)| tool)
            .collect();
        let wanted = call.text("name").map(ToolsetName::new).transpose()?;
        let listed = kernel
            .store()?
            .read(move |tx| {
                let layers = policy.layers_in_force(tx).unwrap_or_default();
                let mut out = Vec::new();
                for toolset in &layers.toolsets {
                    if wanted.as_ref().is_some_and(|name| *name != toolset.name) {
                        continue;
                    }
                    let mut shown = toolset_json(toolset);
                    let selected: Vec<String> = policy
                        .toolset_tools(tx, &toolset.name, &catalog)
                        .unwrap_or_default()
                        .iter()
                        .map(|tool| tool.tool.as_str().to_owned())
                        .collect();
                    if let Json::Object(map) = &mut shown {
                        map.insert("selects".to_owned(), json!(selected));
                    }
                    out.push(shown);
                }
                Ok(out)
            })
            .await?;
        Ok(Output::ok(json!({"toolsets": listed})))
    }
);

// ---- Grants -----------------------------------------------------------------

fn grant_of(call: &Call) -> Result<Grant, Failure> {
    Ok(Grant {
        who: subject_of(call.required("who")?)?,
        scope: scope_of(call.required("scope")?)?,
        max_effect: effect_of(call.required("max_effect")?)?,
        identity_mode: mode_of(call.text("identity_mode").unwrap_or("per_user_session"))?,
    })
}

handler!(
    /// `grant.add`: a grant of the use of tools.
    grant_add,
    |kernel, call| {
        let grant = grant_of(&call)?;
        let what = format!(
            "grant {} on {}",
            subject_text(&grant.who),
            scope_text(&grant.scope)
        );
        change(kernel, &call, "grant.add", what, move |_| {
            Ok(LayerChange::AddGrant(grant))
        })
        .await
    }
);

handler!(
    /// `grant.remove`: takes away the grant for a subject and a scope. When
    /// the set holds two for them at different effect classes, the call
    /// names which.
    grant_remove,
    |kernel, call| {
        let who = subject_of(call.required("who")?)?;
        let scope = scope_of(call.required("scope")?)?;
        let max_effect = call.text("max_effect").map(effect_of).transpose()?;
        let what = format!("grant {} on {}", subject_text(&who), scope_text(&scope));
        change(kernel, &call, "grant.remove", what, move |layers| {
            let mut found: Vec<&Grant> = layers
                .grants
                .iter()
                .filter(|grant| {
                    grant.who == who
                        && grant.scope == scope
                        && max_effect.is_none_or(|effect| grant.max_effect == effect)
                })
                .collect();
            match found.len() {
                0 => Err(codes::input(
                    codes::NOT_FOUND,
                    "No grant is held for that subject and scope.",
                    "Call `grant.list` to see the grants.",
                )),
                1 => Ok(LayerChange::RemoveGrant(found.remove(0).clone())),
                _ => Err(invalid(
                    "Several grants are held for that subject and scope.",
                    "Say which with `max_effect`.",
                )),
            }
        })
        .await
    }
);

handler!(
    /// `grant.list`: the grants, the grants of roles, and who holds which
    /// role, in the set in force.
    grant_list,
    |kernel, call| {
        let _ = &call;
        let policy = policy_of(kernel)?;
        let layers = kernel
            .store()?
            .read(move |tx| Ok(policy.layers_in_force(tx).unwrap_or_default()))
            .await?;
        Ok(Output::ok(json!({
            "grants": layers.grants.iter().map(grant_json).collect::<Vec<_>>(),
            "role_grants": layers.role_grants.iter().map(|grant| json!({
                "role": grant.role.as_str(),
                "scope": scope_text(&grant.scope),
                "max_effect": grant.max_effect.to_string(),
                "identity_mode": grant.identity_mode.as_str(),
            })).collect::<Vec<_>>(),
            "roles": layers.roles.iter().map(|binding| json!({
                "who": subject_text(&binding.who),
                "role": binding.role.as_str(),
            })).collect::<Vec<_>>(),
            "second_admin_publish": layers.second_admin_publish,
        })))
    }
);

// ---- Policy sets ------------------------------------------------------------

/// Reads a policy file inside the call's project folder: a relative path,
/// no `..`, no link out of the folder, and a size the engine takes.
async fn read_policy_file(call: &Call) -> Result<String, Failure> {
    let refuse = |why: &str| {
        codes::input(
            codes::PATH_NOT_ALLOWED,
            format!("The policy file is not read: {why}."),
            "Name a file inside the project folder by a relative path.",
        )
    };
    let folder = call.project_folder()?.to_path_buf();
    let stated = call.required("file")?.to_owned();
    let path = Path::new(&stated);
    if path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(refuse("its path is not a plain relative path"));
    }
    let wanted = folder.join(path);
    let read = tokio::task::spawn_blocking(move || -> Result<String, &'static str> {
        let root = std::fs::canonicalize(&folder).map_err(|_| "the project folder is gone")?;
        let real = std::fs::canonicalize(&wanted).map_err(|_| "it cannot be found")?;
        if !real.starts_with(&root) {
            return Err("it leaves the project folder");
        }
        let meta = std::fs::metadata(&real).map_err(|_| "it cannot be read")?;
        if !meta.is_file() || meta.len() > FILE_LONGEST {
            return Err("it is not a file of a size the engine takes");
        }
        std::fs::read_to_string(&real).map_err(|_| "it is not text")
    })
    .await
    .map_err(|_| refuse("the reading was interrupted"))?;
    read.map_err(refuse)
}

/// The layers and the test table a policy file holds: the layers as JSON,
/// and `tests`, a list of rows.
fn set_of(text: &str, kernel: &Kernel, call: &Call) -> Result<PolicyLayers, Failure> {
    let bad = |why: String| {
        invalid(
            format!("The policy file is not a policy set: {why}."),
            "Write the layers as JSON, with `settings`, and the test table as `tests`.",
        )
    };
    let mut value: Json = serde_json::from_str(text).map_err(|why| bad(why.to_string()))?;
    let rows = match value.as_object_mut().and_then(|map| map.remove("tests")) {
        Some(Json::Array(rows)) => rows,
        Some(_) => return Err(bad("`tests` is a list of rows".to_owned())),
        None => Vec::new(),
    };
    let mut layers: PolicyLayers =
        serde_json::from_value(value).map_err(|why| bad(why.to_string()))?;
    layers.tests = tests_of(&rows, kernel, call)?;
    Ok(layers)
}

handler!(
    /// `policy.test`: runs a policy file's own test table against its own
    /// layers and reports each row that got the wrong answer. Publishes
    /// nothing.
    policy_test,
    |kernel, call| {
        let policy = policy_of(kernel)?;
        let text = read_policy_file(&call).await?;
        let layers = set_of(&text, kernel, &call)?;
        let candidate = policy.compile(&layers)?;
        let report = policy.test(&candidate);
        Ok(Output::ok(json!({
            "ok": report.ok(),
            "passed": report.passed,
            "failed": report.failed.iter().map(|row| json!({
                "name": row.name,
                "expected": row.expected.as_str(),
                "got": row.got.as_str(),
            })).collect::<Vec<_>>(),
        })))
    }
);

handler!(
    /// `policy.publish`: publishes a policy file as the version in force,
    /// after its own tests pass.
    policy_publish,
    |kernel, call| {
        let policy = policy_of(kernel)?;
        let text = read_policy_file(&call).await?;
        let layers = set_of(&text, kernel, &call)?;
        let candidate = policy.compile(&layers)?;
        let review = review_of(kernel, &call, "policy.publish").await?;
        let by = call.caller.principal.principal.clone();
        let proof = proof_of(&call);
        let version = kernel
            .store()?
            .write(a2m_ledger::Durability::Synced, move |tx| {
                policy.publish_reviewed(tx, &candidate, &by, proof, review.as_ref())
            })
            .await?;
        kernel
            .audit(
                &call.caller.principal.principal.id.to_string(),
                "policy.publish",
                "policy set",
                &version.to_string(),
            )
            .await?;
        Ok(published(version, "policy set"))
    }
);

handler!(
    /// `policy.versions`: the history of published versions, oldest first.
    policy_versions,
    |kernel, call| {
        let _ = &call;
        let policy = policy_of(kernel)?;
        let versions = kernel.store()?.read(move |tx| policy.versions(tx)).await?;
        Ok(Output::ok(json!({
            "versions": versions.iter().map(|info| json!({
                "version": info.version.to_string(),
                "published_by": info.published_by,
                "presence": info.presence,
                "published_at": info.published_at.as_unix_millis(),
                "in_force": info.in_force,
            })).collect::<Vec<_>>(),
        })))
    }
);

fn person_ref(text: &str, field: &str) -> Result<PrincipalRef, Failure> {
    text.parse::<PrincipalId>()
        .map(PrincipalRef::person)
        .map_err(|_| {
            invalid(
                format!("`{field}` is not a principal's identifier."),
                "Name a principal by identifier.",
            )
        })
}

handler!(
    /// `policy.tools_allowed`: which published tools a principal may use,
    /// through a toolset where one is named. Records nothing.
    policy_tools_allowed,
    |kernel, call| {
        let policy = policy_of(kernel)?;
        let principal = person_ref(call.required("principal")?, "principal")?;
        let toolset = call.text("toolset").map(ToolsetName::new).transpose()?;
        let catalog = catalog_of(kernel, &call, toolset.as_ref()).await?;
        let allowed = kernel
            .store()?
            .read(move |tx| Ok(policy.tools_allowed(tx, &principal, &catalog)))
            .await?;
        Ok(Output::ok(json!({
            "tools": allowed.iter().map(|tool| tool.tool.as_str()).collect::<Vec<_>>(),
        })))
    }
);

handler!(
    /// `policy.who_can`: which of the principals named may use one published
    /// tool, through a toolset where one is named. Records nothing.
    policy_who_can,
    |kernel, call| {
        let policy = policy_of(kernel)?;
        let wanted = OperationName::new(call.required("tool")?)?;
        let toolset = call.text("toolset").map(ToolsetName::new).transpose()?;
        let candidates: Vec<PrincipalRef> = texts(&call, "candidates")?
            .unwrap_or_default()
            .iter()
            .map(|text| person_ref(text, "candidates"))
            .collect::<Result<_, _>>()?;
        let catalog = catalog_of(kernel, &call, toolset.as_ref()).await?;
        let Some((tool, context)) = catalog.into_iter().find(|(tool, _)| tool.tool == wanted)
        else {
            return Err(codes::input(
                codes::NOT_FOUND,
                format!("No published tool is named `{wanted}`."),
                "Call `tool.search` to find the tool.",
            ));
        };
        let who = kernel
            .store()?
            .read(move |tx| Ok(policy.who_can(tx, &tool, &context, &candidates)))
            .await?;
        Ok(Output::ok(json!({
            "who": who.iter().map(ToString::to_string).collect::<Vec<_>>(),
        })))
    }
);
