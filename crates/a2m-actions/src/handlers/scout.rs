//! The scout (spec section 402; decisions 0170 and 0171): `target.probe`
//! and `auth.detect`, the identify stage; `scout.run`, the fixed pipeline
//! of stages, each stored before the next begins; and `scout.report`.
//!
//! No model is called and nothing is inferred beyond what a rule states.
//! The system is reached only by discovery jobs and drafted-read trials,
//! both in the engine's sandbox, where no intent token exists. A surface
//! the registry has no provider or no discoverer for is reported as such,
//! never guessed about (rule 7). Every text that came from the system is
//! kept under an `untrusted` key and never shown as an instruction
//! (section 502).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use a2m_core::{
    Digest, Environment, Failure, FailureClass, Finding, OperationName, Plan, RunId, SurfaceName,
    TargetName, ToolRuns, Values,
};
use a2m_files::{CompileMode, DRAFT_VERSION, DraftReport, Drafter, TargetRoot};
use a2m_ledger::{Durability, NewUnknown, Owner, SqlValue, Tx, UnknownStatus};
use a2m_provider::{DiscoveryReport, SurfaceDescription};
use serde_json::{Value as Json, json};

use super::build::{off_thread, problems_json, target_folder};
use super::handler;
use super::tools::{Tell, plan_to_observe, run_output, submit_discovery, wait_of};
use crate::codes;
use crate::dispatch::{Call, Next, Output};
use crate::kernel::Kernel;
use crate::tools::FoundTool;

/// How many times the scout tries one drafted read (section 402).
pub(crate) const TRIES: usize = 5;

/// The stages of a scout, by number, as section 402 and the table
/// `actions_scout_stages` number them.
pub(crate) const STAGES: [&str; 8] = [
    "consent_and_budget",
    "identify",
    "discover",
    "compare",
    "draft",
    "check",
    "try",
    "report",
];

/// What one discoverer reported, as the run's control keeps it beside the
/// findings (`actions_discovery_results`).
pub(crate) fn discovery_report_json(report: &Result<DiscoveryReport, Failure>) -> Json {
    match report {
        Ok(report) => json!({
            "coverage": report.coverage.as_str(),
            "bounds_hit": report.bounds_hit,
            "dropped": report.dropped,
        }),
        Err(failure) => json!({
            "failure": {"class": failure.class().as_str(), "code": failure.code().as_str()},
        }),
    }
}

/// Text that came from the system: kept apart and marked, never mixed into
/// what the engine says.
fn untrusted(value: Json) -> Json {
    json!({"untrusted": value})
}

/// What the registry says about one declared surface, before anything is
/// observed.
struct Look {
    name: SurfaceName,
    kind: String,
    archetype: Option<&'static str>,
    provider: Option<String>,
    keeps_a_session: bool,
    discoverers: Vec<Json>,
}

impl Look {
    fn of(kernel: &Kernel, plan: &Plan) -> Vec<Self> {
        let registry = kernel.providers();
        let environment = plan.default_environment();
        plan.surfaces
            .iter()
            .enumerate()
            .map(|(position, surface)| {
                let entry = registry.surface_kind(&surface.kind);
                // The address in the environment a discovery runs in.
                let address = environment
                    .and_then(|environment| {
                        environment
                            .addresses
                            .iter()
                            .find(|over| over.surface.get() == position)
                    })
                    .map_or_else(|| surface.address.clone(), |over| over.address.clone());
                let discoverers = entry
                    .map(|entry| {
                        registry
                            .discoverers_for(&SurfaceDescription {
                                kind: surface.kind.clone(),
                                archetype: entry.archetype,
                                address,
                                facts: Values::new(),
                            })
                            .iter()
                            .map(|handle| {
                                let declared = handle.declared();
                                json!({
                                    "provider": handle.provider().as_str(),
                                    "discoverer": declared.name.as_str(),
                                    "kinds": declared.kinds.iter().map(|kind| kind.as_str()).collect::<Vec<_>>(),
                                    "sources": declared.sources,
                                    "max_level": declared.max_level.as_str(),
                                    "bounds": {
                                        "seconds": declared.bounds.seconds,
                                        "bytes": declared.bounds.bytes,
                                        "depth": declared.bounds.depth,
                                    },
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                Self {
                    name: surface.name.clone(),
                    kind: surface.kind.to_string(),
                    archetype: entry.map(|entry| entry.archetype.as_str()),
                    provider: entry.map(|entry| entry.provider.to_string()),
                    keeps_a_session: entry.is_some_and(|entry| entry.session),
                    discoverers,
                }
            })
            .collect()
    }
}

/// One row of `actions_discovery_results`.
struct Reported {
    provider: String,
    discoverer: String,
    findings: Vec<Digest>,
    report: Json,
}

fn discovery_results(tx: &mut Tx<'_>, run: RunId) -> Result<Vec<Reported>, Failure> {
    let rows = tx.tables(Owner::Actions).query(
        "SELECT provider, discoverer, findings, report FROM actions_discovery_results
         WHERE run_id = ?1 ORDER BY provider, discoverer",
        &[SqlValue::from(run.to_string())],
    )?;
    let mut out = Vec::new();
    for row in rows {
        let findings: Vec<String> = serde_json::from_str(row.text(2)?).unwrap_or_default();
        out.push(Reported {
            provider: row.text(0)?.to_owned(),
            discoverer: row.text(1)?.to_owned(),
            findings: findings.iter().filter_map(|id| id.parse().ok()).collect(),
            report: serde_json::from_str(row.text(3)?).unwrap_or(Json::Null),
        });
    }
    Ok(out)
}

/// The state of a surface's identification, from how its discovery ended.
fn identified_state(state: a2m_core::RunState, failure: Option<FailureClass>) -> &'static str {
    if !state.is_final() {
        return "working";
    }
    match failure {
        None => "identified",
        Some(FailureClass::SessionExpired) => "needs_login",
        Some(FailureClass::Policy | FailureClass::EdgeBlocked) => "refused",
        Some(_) => "failed",
    }
}

/// What the identify stage came to.
struct Identified {
    plan_hash: Digest,
    /// One entry per surface, with no run identifier in it.
    surfaces: Vec<Json>,
    /// The discovery run of each surface that has one.
    runs: BTreeMap<String, RunId>,
    /// Every discovery ended.
    complete: bool,
    /// Why nothing was observed, when the drafts do not compile.
    not_compiled: Option<String>,
}

/// The identify stage: what the registry says of each declared surface, and
/// one discovery job per surface that has a discoverer, in the sandbox. A
/// surface with no provider or no discoverer is reported, and nothing runs
/// for it.
async fn identify(
    kernel: &Kernel,
    call: &Call,
    target: &TargetName,
    only: Option<&str>,
    to_its_end: bool,
) -> Result<Identified, Failure> {
    let (plan_hash, plan) = plan_to_observe(kernel, call, target).await?;
    let looks: Vec<Look> = Look::of(kernel, &plan)
        .into_iter()
        .filter(|look| only.is_none_or(|wanted| look.name.as_str() == wanted))
        .collect();
    if looks.is_empty() && only.is_some() {
        return Err(codes::input(
            codes::NOT_FOUND,
            format!("The target `{target}` declares no such surface."),
            "Name a surface the target's files declare, or leave `surface` out.",
        ));
    }
    let environment: Option<Environment> = plan
        .default_environment()
        .map(|environment| environment.name.clone());
    let mut runs = BTreeMap::new();
    for look in &looks {
        if look.provider.is_some()
            && !look.discoverers.is_empty()
            && let Some(environment) = &environment
        {
            let run =
                submit_discovery(kernel, call, target, plan_hash, environment, &look.name).await?;
            runs.insert(look.name.to_string(), run);
        }
    }
    let workspace = call.caller.principal.workspace;
    let mut surfaces = Vec::new();
    let mut complete = true;
    for look in looks {
        // The scout waits for a discovery to its end, as long as its
        // discoverers may take by their manifests, and never leaves one in
        // hand behind it (see `TRIAL_BOUND`); `target.probe` waits as its
        // caller asked.
        let wait = if to_its_end {
            let declared: u64 = look
                .discoverers
                .iter()
                .filter_map(|discoverer| discoverer["bounds"]["seconds"].as_u64())
                .sum();
            Duration::from_secs(declared.saturating_add(30))
        } else {
            wait_of(call)
        };
        let mut entry = json!({
            "surface": look.name.as_str(),
            "kind": look.kind,
            "archetype": look.archetype,
            "provider": look.provider,
            "keeps_a_session": look.keeps_a_session,
            "discoverers": look.discoverers,
        });
        let state = match runs.get(look.name.as_str()) {
            None if look.provider.is_none() => {
                entry["why"] = json!(
                    "No enabled provider serves this surface kind, so nothing can be said of it."
                );
                "no_provider"
            }
            None if environment.is_none() => {
                entry["why"] = json!("The target declares no environment to observe it in.");
                "not_observed"
            }
            None => {
                entry["why"] =
                    json!("No enabled discoverer suits this surface; it is not guessed about.");
                "no_discoverer"
            }
            Some(run) => {
                let run = *run;
                let row = kernel.wait_for_rest(run, wait).await?;
                complete &= row.state.is_final();
                if let Some((class, code)) = row.failure.as_ref() {
                    entry["failure"] = json!({"class": class.as_str(), "code": code.as_str()});
                }
                let reported = kernel
                    .store()?
                    .read(move |tx| discovery_results(tx, run))
                    .await?;
                let named = target.clone();
                let ids: Vec<Digest> = reported
                    .iter()
                    .flat_map(|one| one.findings.iter().copied())
                    .collect();
                let found = kernel
                    .store()?
                    .read(move |tx| {
                        let mut found = Vec::new();
                        for id in &ids {
                            if let Some(finding) = tx.findings().get(&workspace, &named, id)? {
                                found.push(finding);
                            }
                        }
                        Ok(found)
                    })
                    .await?;
                let mut by_kind = BTreeMap::<String, BTreeMap<String, u64>>::new();
                for finding in &found {
                    *by_kind
                        .entry(finding.kind().to_string())
                        .or_default()
                        .entry(finding.level().as_str().to_owned())
                        .or_default() += 1;
                }
                entry["findings_by_kind"] = json!(by_kind);
                entry["ended"] = Json::Array(
                    reported
                        .iter()
                        .map(|one| {
                            json!({
                                "provider": one.provider,
                                "discoverer": one.discoverer,
                                "findings": one.findings.len(),
                                "report": one.report,
                            })
                        })
                        .collect(),
                );
                identified_state(row.state, row.failure.as_ref().map(|(class, _)| *class))
            }
        };
        entry["state"] = json!(state);
        surfaces.push(entry);
    }
    Ok(Identified {
        plan_hash,
        surfaces,
        runs,
        complete,
        not_compiled: None,
    })
}

impl Identified {
    fn runs_json(&self) -> Json {
        json!(
            self.runs
                .iter()
                .map(|(surface, run)| (surface.clone(), run.to_string()))
                .collect::<BTreeMap<_, _>>()
        )
    }
}

handler!(
    /// `target.probe`: the identify stage on its own. What the registry
    /// says of each declared surface, and one discovery job per surface
    /// that has a discoverer, in the engine's sandbox: which provider
    /// answers, what its discoverers found and at which level, the bounds
    /// they reached, and what refused. Never writes.
    target_probe,
    |kernel, call| {
        let target = TargetName::new(call.required("target")?)?;
        let identified = identify(kernel, &call, &target, call.text("surface"), false).await?;
        let output = Output::ok(json!({
            "target": target.as_str(),
            "plan_hash": identified.plan_hash.to_string(),
            "surfaces": identified.surfaces,
            "runs": identified.runs_json(),
            "complete": identified.complete,
        }));
        Ok(if identified.complete {
            output.then(Next::new(
                "auth.detect",
                json!({"target": target.as_str()}),
                "to see how each surface signs in",
            ))
        } else {
            output.then(Next::new(
                "target.probe",
                json!({"target": target.as_str()}),
                "when the discovery runs have ended",
            ))
        })
    }
);

/// The sign-in entries the target's files declare for each surface, as
/// `auth.yaml` writes them: the identity mode and the provider. `None` when
/// the call is made outside a project that holds the target.
async fn declared_sign_ins(
    kernel: &Kernel,
    call: &Call,
    target: &TargetName,
) -> Result<Option<BTreeMap<String, Vec<Json>>>, Failure> {
    let Ok(folder) = call.project_folder() else {
        return Ok(None);
    };
    let (kernel, folder, target) = (kernel.clone(), folder.to_path_buf(), target.clone());
    off_thread(move || {
        let folder = target_folder(&folder, &target)?;
        if !folder.is_dir() {
            return Ok(None);
        }
        let set = kernel.providers().compiler_set();
        let Ok(sources) = a2m_files::load(&TargetRoot::new(folder.as_path()), set) else {
            return Ok(Some(BTreeMap::new()));
        };
        let mut declared: BTreeMap<String, Vec<Json>> = BTreeMap::new();
        for doc in sources.docs_of("auth") {
            let whole = doc.to_json();
            for entry in whole["auth"].as_array().into_iter().flatten() {
                for surface in entry["surfaces"].as_array().into_iter().flatten() {
                    if let Some(surface) = surface.as_str() {
                        declared.entry(surface.to_owned()).or_default().push(json!({
                            "mode": entry["mode"],
                            "provider": entry["provider"],
                        }));
                    }
                }
            }
        }
        Ok(Some(declared))
    })
    .await
}

/// How each identified surface signs in, as claims for a person to
/// confirm: what its kind's pack registered, what the target's files
/// declare, and what its discovery met. Never a credential.
fn sign_ins(identified: &Identified, declared: Option<&BTreeMap<String, Vec<Json>>>) -> Vec<Json> {
    identified
        .surfaces
        .iter()
        .map(|surface| {
            let name = surface["surface"].as_str().unwrap_or_default();
            let observed = match surface["state"].as_str() {
                Some("identified") => "answered",
                Some("needs_login") => "needs_sign_in",
                Some(state) => state,
                None => "not_observed",
            };
            let declared_here = declared.map(|declared| {
                declared.get(name).cloned().unwrap_or_default()
            });
            let first = declared_here.as_ref().and_then(|entries| entries.first());
            let mut unknown = Vec::new();
            if first.is_none() {
                unknown.push("identity_mode");
                unknown.push("method");
            }
            json!({
                "surface": name,
                "keeps_a_session": surface["keeps_a_session"],
                "declared": declared_here,
                "observed": observed,
                "failure": surface.get("failure"),
                "method": first.map(|entry| entry["provider"].clone()),
                "identity_mode": first.map(|entry| entry["mode"].clone()),
                "level": if first.is_some() { "declared" } else { "unknown" },
                "unknown": unknown,
                "confirm": "A claim for a person to confirm by signing in; the engine never holds a credential it was told here.",
            })
        })
        .collect()
}

handler!(
    /// `auth.detect`: how each surface signs in, as claims to be confirmed
    /// by a person: whether its kind keeps a session, what the target's
    /// files declare, and what its discovery met. No rule recommends an
    /// identity mode the files do not declare, so none is guessed.
    auth_detect,
    |kernel, call| {
        let target = TargetName::new(call.required("target")?)?;
        let identified = identify(kernel, &call, &target, call.text("surface"), false).await?;
        let declared = declared_sign_ins(kernel, &call, &target).await?;
        let shown = sign_ins(&identified, declared.as_ref());
        let needs: Vec<&Json> = shown
            .iter()
            .filter(|surface| surface["observed"] == "needs_sign_in")
            .collect();
        let output = Output::ok(json!({
            "target": target.as_str(),
            "surfaces": shown,
            "runs": identified.runs_json(),
            "complete": identified.complete,
        }));
        Ok(if needs.is_empty() {
            output.then(Next::new(
                "scout.run",
                json!({"target": target.as_str()}),
                "to draft what follows from what was found",
            ))
        } else {
            output.then(Next::new(
                "session.login",
                json!({"target": target.as_str()}),
                "a person signs in; then scout again",
            ))
        })
    }
);

// ---- the scout ---------------------------------------------------------

/// One scout, as `actions_scouts` holds it.
#[derive(Debug, Clone)]
struct Scout {
    id: RunId,
}

/// The key a scout is kept under beside its person and target: the
/// digest of the project folder it was made in, as the dispatcher resolved
/// it, or `none` outside a project. A target of one name in two projects
/// is two targets, and one's scout is never continued in the other (audit
/// finding A3).
pub(crate) fn project_key(call: &Call) -> String {
    call.project_folder().map_or_else(
        |_| "none".to_owned(),
        |folder| Digest::of(folder.to_string_lossy().as_bytes()).to_string(),
    )
}

/// The newest scout of a target by the caller's person in one project: the
/// unfinished one to resume, or with `finished`, the newest that finished.
/// The project a scout was made in is stored with its first stage; a scout
/// with none stored is of no project, and is never resumed.
fn newest_scout(
    tx: &mut Tx<'_>,
    workspace: &a2m_core::WorkspaceId,
    principal: &a2m_core::PrincipalId,
    target: &TargetName,
    project: &str,
    finished: bool,
) -> Result<Option<Scout>, Failure> {
    let rows = tx.tables(Owner::Actions).query(
        "SELECT s.scout_id, s.finished_at, g.result FROM actions_scouts s
         LEFT JOIN actions_scout_stages g ON g.run_id = s.scout_id AND g.stage = 0
         WHERE s.workspace_id = ?1 AND s.principal_id = ?2 AND s.target = ?3
         ORDER BY s.started_at DESC, s.scout_id DESC",
        &[
            SqlValue::from(workspace.to_string()),
            SqlValue::from(principal.to_string()),
            SqlValue::from(target.as_str()),
        ],
    )?;
    for row in rows {
        let first: Json = row
            .opt_text(2)?
            .and_then(|text| serde_json::from_str(text).ok())
            .unwrap_or(Json::Null);
        if first["project"].as_str() != Some(project) {
            continue;
        }
        let done = row.opt_int(1)?.is_some();
        if done == finished {
            return Ok(Some(Scout {
                id: row.text(0)?.parse()?,
            }));
        }
        if !finished {
            // The newest scout finished: there is none to resume.
            return Ok(None);
        }
    }
    Ok(None)
}

/// What the newest finished scout of the target by the caller's person, in
/// the call's project, found of one drafted read: its tried entry when it
/// was proven at `executed`, with the scout's identifier.
pub(crate) async fn proven_by_scout(
    kernel: &Kernel,
    call: &Call,
    target: &TargetName,
    tool: &str,
) -> Result<Option<Json>, Failure> {
    let principal = &call.caller.principal;
    let (workspace, person) = (principal.workspace, principal.principal.id);
    let (named, project) = (target.clone(), project_key(call));
    let stages = kernel
        .store()?
        .read(move |tx| {
            let Some(scout) = newest_scout(tx, &workspace, &person, &named, &project, true)? else {
                return Ok(None);
            };
            Ok(Some((scout.id, stored_stages(tx, scout.id)?)))
        })
        .await?;
    let Some((scout, stages)) = stages else {
        return Ok(None);
    };
    Ok(stages
        .get(&6)
        .and_then(|tried| tried["tools"].as_array())
        .into_iter()
        .flatten()
        .find(|entry| {
            entry["tool"] == tool && entry["state"] == "proven" && entry["level"] == "executed"
        })
        .map(|entry| {
            json!({
                "by": "scout",
                "scout_id": scout.to_string(),
                "level": "executed",
                "tries": entry["tries"],
            })
        }))
}

fn stored_stages(tx: &mut Tx<'_>, scout: RunId) -> Result<BTreeMap<u8, Json>, Failure> {
    let rows = tx.tables(Owner::Actions).query(
        "SELECT stage, result FROM actions_scout_stages WHERE run_id = ?1 ORDER BY stage",
        &[SqlValue::from(scout.to_string())],
    )?;
    let mut out = BTreeMap::new();
    for row in rows {
        let stage = u8::try_from(row.int(0)?).unwrap_or(u8::MAX);
        out.insert(
            stage,
            serde_json::from_str(row.text(1)?).unwrap_or(Json::Null),
        );
    }
    Ok(out)
}

/// The hash of a stage's input: the result of the stage before it.
fn input_hash(previous: &Json) -> Digest {
    Digest::of(previous.to_string().as_bytes())
}

impl Kernel {
    /// Stores one finished stage of a scout; the report stage also marks
    /// the scout finished with the report's hash.
    async fn store_stage(
        &self,
        scout: RunId,
        stage: u8,
        given: Digest,
        result: &Json,
    ) -> Result<(), Failure> {
        let redacted = crate::dispatch::redact_json(self.redact().as_ref(), result);
        let text = redacted.to_string();
        let hash = redacted["report_hash"].as_str().map(str::to_owned);
        self.store()?
            .write(Durability::Synced, move |tx| {
                let now = tx.now().as_unix_millis();
                let mut tables = tx.tables(Owner::Actions);
                tables.execute(
                    "INSERT INTO actions_scout_stages (run_id, stage, input_hash, result, finished_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT (run_id, stage) DO NOTHING",
                    &[
                        SqlValue::from(scout.to_string()),
                        SqlValue::from(i64::from(stage)),
                        SqlValue::from(given.to_string()),
                        SqlValue::from(text),
                        SqlValue::from(now),
                    ],
                )?;
                if let Some(hash) = hash {
                    tables.execute(
                        "UPDATE actions_scouts SET finished_at = ?2, report_hash = ?3
                         WHERE scout_id = ?1",
                        &[
                            SqlValue::from(scout.to_string()),
                            SqlValue::from(now),
                            SqlValue::from(hash),
                        ],
                    )?;
                }
                Ok(())
            })
            .await
    }
}

/// Opens each question in the unknowns ledger that is not open there
/// already, so that a scout run again, or resumed, asks it once.
fn open_questions(
    tx: &mut Tx<'_>,
    workspace: a2m_core::WorkspaceId,
    target: &TargetName,
    asked: &[(String, String, String, String)],
) -> Result<(), Failure> {
    let open: BTreeSet<(String, String)> = tx
        .findings()
        .unknowns(&workspace, target)?
        .into_iter()
        .filter(|row| row.status != UnknownStatus::Settled)
        .map(|row| (row.question, row.scope))
        .collect();
    for (question, scope, why, settled_by) in asked {
        if open.contains(&(question.clone(), scope.clone())) {
            continue;
        }
        tx.findings().open_question(NewUnknown {
            workspace,
            target: target.clone(),
            question: question.clone(),
            scope: scope.clone(),
            why_it_matters: why.clone(),
            settled_by_what: settled_by.clone(),
        })?;
    }
    Ok(())
}

/// The findings an identify stage's runs made, in a fixed order.
async fn findings_of(
    kernel: &Kernel,
    workspace: a2m_core::WorkspaceId,
    target: &TargetName,
    runs: &BTreeMap<String, String>,
) -> Result<(Vec<Finding>, Vec<Json>), Failure> {
    let runs: Vec<(String, RunId)> = runs
        .iter()
        .filter_map(|(surface, run)| Some((surface.clone(), run.parse().ok()?)))
        .collect();
    let target = target.clone();
    kernel
        .store()?
        .read(move |tx| {
            let mut findings = BTreeMap::<Digest, Finding>::new();
            let mut by_discoverer = Vec::new();
            for (surface, run) in &runs {
                for reported in discovery_results(tx, *run)? {
                    for id in &reported.findings {
                        if let Some(finding) = tx.findings().get(&workspace, &target, id)? {
                            findings.insert(*id, finding);
                        }
                    }
                    by_discoverer.push(json!({
                        "surface": surface,
                        "provider": reported.provider,
                        "discoverer": reported.discoverer,
                        "findings": reported.findings.iter().map(Digest::to_string).collect::<Vec<_>>(),
                        "report": reported.report,
                    }));
                }
            }
            Ok((findings.into_values().collect(), by_discoverer))
        })
        .await
}

/// The target's folder in the call's project, when there is one.
async fn folder_of(call: &Call, target: &TargetName) -> Result<Option<PathBuf>, Failure> {
    let Ok(project) = call.project_folder() else {
        return Ok(None);
    };
    let (project, target) = (project.to_path_buf(), target.clone());
    off_thread(move || {
        let folder = target_folder(&project, &target)?;
        Ok(folder.is_dir().then(|| folder.as_path().to_path_buf()))
    })
    .await
}

/// The most files the compare stage copies from a target's folder.
pub(crate) const COPY_MOST_FILES: usize = 4_096;
/// The most bytes it copies, all files together.
pub(crate) const COPY_MOST_BYTES: u64 = 64 * 1024 * 1024;

/// The folder under the engine's cache folder where the compare stage
/// copies a target's folder, one folder per scout. Nothing else is kept
/// there: whatever a stopped engine left in it is removed when the next one
/// opens (audit finding A5).
pub(crate) const SCOUT_SCRATCH: &str = "scout";

fn too_much() -> Failure {
    codes::input(
        codes::PATH_NOT_ALLOWED,
        format!(
            "The target's folder holds more than {COPY_MOST_FILES} files or {COPY_MOST_BYTES} bytes, so it is not copied to be compared."
        ),
        "Keep only the target's own files in its folder.",
    )
}

/// Copies the regular files of a target's folder, and its folders, into an
/// empty folder of the engine's cache, within `COPY_MOST_FILES` and
/// `COPY_MOST_BYTES`. The target's folder is read through checked paths
/// (audit finding A5): a link is never followed, whether it stood at the
/// name when the folder was listed or was put there since, since the file
/// is opened without following one; a hidden name is passed over. A copy
/// is made new, never through what stands at its name.
fn copy_target(
    from: &a2m_provider::CheckedPath,
    to: &Path,
    budget: &mut (usize, u64),
) -> Result<(), Failure> {
    let unavailable = || {
        codes::input(
            codes::PATH_NOT_ALLOWED,
            "The target's folder could not be copied to compare it.",
            "Check that the target's folder may be read by this user.",
        )
    };
    std::fs::create_dir_all(to).map_err(|_| unavailable())?;
    let (entries, _) = from.list()?;
    for entry in entries {
        let Some(name) = entry.relative().rsplit('/').next().map(str::to_owned) else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        match entry.entry_kind()? {
            a2m_provider::EntryKind::Folder => copy_target(&entry, &to.join(&name), budget)?,
            a2m_provider::EntryKind::File => {
                budget.0 += 1;
                if budget.0 > COPY_MOST_FILES {
                    return Err(too_much());
                }
                let left = COPY_MOST_BYTES.saturating_sub(budget.1);
                let bytes = entry
                    .read(left.min(a2m_files::LARGEST_TARGET_FILE as u64))
                    .map_err(|_| too_much())?;
                budget.1 = budget.1.saturating_add(bytes.len() as u64);
                let mut copy = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(to.join(&name))
                    .map_err(|_| unavailable())?;
                std::io::Write::write_all(&mut copy, &bytes).map_err(|_| unavailable())?;
            }
            _ => {}
        }
    }
    Ok(())
}

/// Runs the drafter over `findings` in `folder`.
fn draft_into(
    kernel: &Kernel,
    folder: &Path,
    findings: &[Finding],
) -> Result<DraftReport, Failure> {
    let set = kernel.providers().compiler_set();
    let root = TargetRoot::new(folder);
    let redact = kernel.redact();
    Drafter::new(set, &root)
        .with_redact(redact.as_ref())
        .draft(findings)
}

fn paths(list: &[a2m_files::RelPath]) -> Vec<String> {
    list.iter().map(|path| path.as_str().to_owned()).collect()
}

/// The compare stage: what the observations draft by rule, set against
/// what the target's files hold. The drafter is run over a copy of the
/// target's folder in the engine's cache, so nothing in the project changes; a file it would
/// write where the target holds one a person wrote is a difference, and
/// each difference is an open question. A finding no rule covers is
/// listed, since nothing in the files can be compared with it by rule.
async fn compare(
    kernel: &Kernel,
    call: &Call,
    target: &TargetName,
    scout: RunId,
    findings: Vec<Finding>,
) -> Result<(Json, Vec<(String, String, String, String)>), Failure> {
    let kernel = kernel.clone();
    let (project, target) = (call.project_folder()?.to_path_buf(), target.clone());
    off_thread(move || {
        // The engine's own cache folder: no other account writes there,
        // and the name is the scout's, so no two scouts share it.
        let scratch = kernel
            .paths()
            .cache
            .join(SCOUT_SCRATCH)
            .join(scout.to_string());
        let _ = std::fs::remove_dir_all(&scratch);
        let from = target_folder(&project, &target)?;
        let copied = copy_target(&from, &scratch, &mut (0, 0));
        let drafted = copied.and_then(|()| draft_into(&kernel, &scratch, &findings));
        let _ = std::fs::remove_dir_all(&scratch);
        let report = drafted?;
        let mut asked = Vec::new();
        for path in &report.conflicts {
            asked.push((
                format!(
                    "The system, as observed, drafts `{path}` otherwise than the target's file holds it. Which is right?"
                ),
                path.as_str().to_owned(),
                "The file is the target's, and what was observed differs from it.".to_owned(),
                "A person reads the file beside the findings it would be drafted from (`finding.describe`).".to_owned(),
            ));
        }
        Ok((
            json!({
                "differs": paths(&report.conflicts),
                "would_draft": paths(&report.written),
                "not_covered": report.not_covered.iter().map(Digest::to_string).collect::<Vec<_>>(),
            }),
            asked,
        ))
    })
    .await
}

/// The draft stage: the drafter in the target's folder. It writes only new
/// files and its own unchanged drafts, never a file a person wrote, and
/// never a write.
async fn draft(
    kernel: &Kernel,
    folder: PathBuf,
    findings: Vec<Finding>,
) -> Result<(Json, Vec<(String, String, String, String)>), Failure> {
    let kernel = kernel.clone();
    off_thread(move || {
        let report = draft_into(&kernel, &folder, &findings)?;
        let failing: BTreeMap<String, Vec<Json>> = report
            .kept_failing
            .iter()
            .map(|(path, problems)| (path.as_str().to_owned(), marked_problems(problems)))
            .collect();
        let asked: Vec<(String, String, String, String)> = report
            .open_questions
            .iter()
            .map(|open| {
                (
                    // The product's sentence and the system's words, kept
                    // apart in what is stored (decision 0193).
                    open.stored(),
                    open.file.as_str().to_owned(),
                    format!("The draft `{}` does not compile until it is answered.", open.file),
                    "A person states the value; the system did not.".to_owned(),
                )
            })
            .collect();
        let files: Vec<Json> = report
            .written
            .iter()
            .map(|path| {
                json!({
                    "file": path.as_str(),
                    "state": if failing.contains_key(path.as_str()) { "fails" } else { "compiles" },
                    "problems": failing.get(path.as_str()).cloned().unwrap_or_default(),
                })
            })
            .collect();
        // The drafts that hold an open question, each once: the try stage
        // compiles without them (decision 0180).
        let held_back: BTreeSet<String> = report
            .open_questions
            .iter()
            .map(|open| open.file.as_str().to_owned())
            .collect();
        Ok((
            json!({
                "written": files,
                "held_back": held_back,
                "conflicts": paths(&report.conflicts),
                "refused": report.refused.iter().map(|(path, why)| json!({"file": path.as_str(), "why": why})).collect::<Vec<_>>(),
                "open_questions": report.open_questions.len(),
            }),
            asked,
        ))
    })
    .await
}

/// The drafted operations among `written`: their tool names, by file.
async fn drafted_tools(
    kernel: &Kernel,
    folder: PathBuf,
    written: Vec<String>,
    held_back: Vec<String>,
) -> Result<Vec<(String, String)>, Failure> {
    let kernel = kernel.clone();
    off_thread(move || {
        let set = kernel.providers().compiler_set();
        let Ok(sources) = a2m_files::load(&TargetRoot::new(&folder), set) else {
            return Ok(Vec::new());
        };
        let mut tools = Vec::new();
        for file in sources.of_kind("operation") {
            if !written.iter().any(|path| path == file.path.as_str())
                || held_back.iter().any(|path| path == file.path.as_str())
            {
                continue;
            }
            for doc in &file.docs {
                if let Some(name) = doc.text("operation") {
                    tools.push((name.to_owned(), file.path.as_str().to_owned()));
                }
            }
        }
        tools.sort();
        Ok(tools)
    })
    .await
}

/// How long the scout waits for one trial to end, counted from when it was
/// submitted, whatever the call's `wait_seconds`. A trial in hand is waited
/// for to its end, so that a call never leaves one behind it: an engine
/// that runs inside a command ends with the command, and a trial it had
/// claimed would wait out its lease before another engine could take it.
pub(crate) const TRIAL_BOUND: Duration = Duration::from_secs(120);

/// What the scout tries a drafted read with.
enum TryInput {
    /// Its first example, or nothing when it needs no input.
    Stated(Values),
    /// The first key a list read of the same record answered in this scout,
    /// by the list's tool and output. The value is kept only in the
    /// trial's own stored call, never in the report.
    FromList(Values, String, String),
    /// Nothing the scout may use, and why.
    None(&'static str),
}

/// The digest that makes one trial of a scout the run it is: the scout, the
/// plan tried, the read, which try it is, and the input. A continuation of
/// the scout finds the same trial by it, ended or not, and submits none
/// again (ledger rule 6).
fn trial_hash(scout: RunId, plan: Digest, try_number: usize, input: &Values) -> Digest {
    let mut hasher = a2m_core::FieldHasher::new();
    hasher.field("purpose", b"anything2mcp scout trial 1");
    hasher.field("scout", scout.to_string().as_bytes());
    hasher.field("plan", plan.to_string().as_bytes());
    hasher.field("try", try_number.to_string().as_bytes());
    hasher.field("input", input.digest().to_string().as_bytes());
    hasher.finish()
}

/// The trial of `found` at `try_number` in this scout: the run submitted for
/// it before, by an earlier call of the same scout, or a new one, and
/// whether it is new.
async fn trial_of(
    kernel: &Kernel,
    call: &Call,
    scout: RunId,
    found: &FoundTool,
    input: &Values,
    try_number: usize,
    environment: &Environment,
) -> Result<(RunId, bool), Failure> {
    let hash = trial_hash(scout, found.plan_hash, try_number, input);
    let principal = &call.caller.principal;
    let (workspace, person) = (principal.workspace, principal.principal.id);
    let (tool, at) = (found.tool.name.clone(), environment.clone());
    let earlier = kernel
        .store()?
        .read(move |tx| {
            tx.ledger().run_called_since(
                &workspace,
                &person,
                (&tool, &at, &hash),
                a2m_core::Timestamp::from_unix_millis(0),
            )
        })
        .await?;
    match earlier {
        Some(run) => Ok((run, false)),
        None => {
            super::tools::submit_trial_as(kernel, call, found, input, environment.clone(), hash)
                .await
                .map(|run| (run, true))
        }
    }
}

/// How one trial came to rest: its run's row, or `None` when it had not
/// ended within its bound.
async fn trial_rests(kernel: &Kernel, run: RunId) -> Result<Option<a2m_ledger::RunRow>, Failure> {
    let row = kernel.store()?.read(move |tx| tx.ledger().run(run)).await?;
    let now = kernel.inner.clock.now().as_unix_millis();
    let waited = u64::try_from(now.saturating_sub(row.created_at.as_unix_millis())).unwrap_or(0);
    let left = TRIAL_BOUND.saturating_sub(Duration::from_millis(waited));
    let row = kernel.wait_for_rest(run, left).await?;
    // At rest: ended, or stopped or waiting with no job in hand for it.
    let active = matches!(
        row.state,
        a2m_core::RunState::Queued | a2m_core::RunState::Validating | a2m_core::RunState::Running
    );
    if row.state.is_final() || !active {
        return Ok(Some(row));
    }
    // Not ended within its bound: it is cancelled, so that it does not run
    // later for nobody; a trial only reads.
    let _ = kernel.sched()?.cancel(run).await;
    Ok(None)
}

/// The input a read takes from a list read of the same record that was
/// proven in this scout. The rule, and no other: the read states exactly
/// one key input; among the reads proven so far in this scout run, exactly
/// one acts on the same surface (by its first binding) and declares an
/// output of exactly the key's name whose type is a list; the first item of
/// that output, in the list's own order, is text or a whole number.
/// Anything else is not tried, with its reason (rule 7: no guessing by
/// resemblance).
fn key_from_list(operation: &a2m_core::OperationPlan, proven: &[Proven]) -> TryInput {
    let [key] = operation.identity.key.as_slice() else {
        return TryInput::None(
            "It takes an input it states no example of, and its key is not one input; the scout has no value for it.",
        );
    };
    let Some(key) = operation.inputs.get(key.get()) else {
        return TryInput::None(
            "It takes an input it states no example of; the scout has no value for it.",
        );
    };
    if operation
        .inputs
        .iter()
        .any(|input| input.name != key.name && input.required && input.default.is_none())
    {
        return TryInput::None(
            "It takes an input besides its key that it states no example of; the scout has no value for it.",
        );
    }
    let Some(surface) = operation.bindings.first().map(|binding| binding.surface) else {
        return TryInput::None("It has no binding to try.");
    };
    let lists: Vec<&Proven> = proven
        .iter()
        .filter(|read| {
            read.surface == surface
                && read.lists.contains(&key.name)
                && read.result.get(&key.name).is_some_and(Json::is_array)
        })
        .collect();
    let [
        Proven {
            tool: list, result, ..
        },
    ] = lists.as_slice()
    else {
        return TryInput::None(if lists.is_empty() {
            "It takes a key it states no example of, and no list read of the same surface proven in this scout answers an output of the key's name."
        } else {
            "It takes a key it states no example of, and more than one list read proven in this scout answers an output of the key's name; nothing says which is of the same record."
        });
    };
    let value = match result[&key.name].get(0) {
        Some(Json::String(text)) => a2m_core::Value::Text(text.clone()),
        Some(Json::Number(number)) if number.is_i64() => {
            a2m_core::Value::Int(number.as_i64().unwrap_or_default())
        }
        _ => {
            return TryInput::None(
                "It takes a key it states no example of, and the list read of the same record answered no first key that is text or a whole number.",
            );
        }
    };
    TryInput::FromList(
        Values::new().with(&key.name, value),
        list.clone(),
        key.name.clone(),
    )
}

/// A read proven in this scout run, as the next reads may use it: kept in
/// memory for one call only.
struct Proven {
    tool: String,
    /// The surface its first binding acts on.
    surface: a2m_core::SurfaceIndex,
    /// Its outputs whose declared type is a list.
    lists: Vec<String>,
    /// Its first result.
    result: Json,
}

/// What the try stage came to in one call.
struct Tried {
    result: Json,
    runs: BTreeMap<String, Vec<String>>,
    /// Every trial was made and came to rest, or passed its bound.
    complete: bool,
}

/// The try stage: each drafted read tried in the sandbox, `TRIES` times,
/// one after another, and the results compared. Each trial is submitted
/// once and found again by a continuation (`trial_of`); one in hand is
/// waited for to its end, or to `TRIAL_BOUND`; one that fails ends the
/// read's tries, and the read is tried and not proven, with its named
/// failure. The call's `wait_seconds` is looked at only between trials.
/// The reads that take a key they state no example of are tried last, with
/// the first key a list read of the same record answered (`key_from_list`).
#[allow(
    clippy::too_many_lines,
    reason = "one stage, in the order of its steps; split, its state would pass through every part"
)]
async fn try_reads(
    kernel: &Kernel,
    call: &Call,
    scout: RunId,
    target: &TargetName,
    folder: PathBuf,
    tools: &[(String, String)],
    held_back: Vec<String>,
) -> Result<Tried, Failure> {
    let mut tried = Vec::new();
    let mut runs = BTreeMap::new();
    let left_out = json!(held_back);
    if tools.is_empty() {
        return Ok(Tried {
            result: json!({"tools": tried, "left_out": left_out}),
            runs,
            complete: true,
        });
    }
    // What can compile is compiled: every file of the target but the
    // drafts that hold an open question, since one such file keeps its
    // whole kind from compiling (decision 0180).
    let compiled = {
        let kernel = kernel.clone();
        off_thread(move || Ok(super::build::compile_what_can(&kernel, &folder, &held_back))).await?
    };
    let (compiled, left_out) = match compiled {
        Ok((compiled, left_out)) => (compiled, json!(left_out)),
        Err(problems) => {
            for (tool, file) in tools {
                tried.push(json!({"tool": tool, "file": file, "state": "not_tried", "why": "The drafts do not compile, without those that hold an open question."}));
            }
            return Ok(Tried {
                result: json!({"tools": tried, "left_out": left_out, "problems": marked_problems(&problems)}),
                runs,
                complete: true,
            });
        }
    };
    let plan = Arc::new(compiled.plan);
    kernel.keep_draft(target, compiled.encoded.hash, Arc::clone(&plan));
    let environment = plan
        .default_environment()
        .map(|environment| environment.name.clone());
    let until = tokio::time::Instant::now() + wait_of(call);
    let mut waited = false;
    // The reads proven so far, kept in memory for this call only.
    let mut proven: Vec<Proven> = Vec::new();
    // Reads with a stated input first; then those that take a key they
    // state no example of, which may take it from a list proven before.
    let mut order: Vec<(bool, &String, &String)> = Vec::new();
    for (name, file) in tools {
        let needs_key = OperationName::new(name)
            .ok()
            .and_then(|operation| plan.tool(&operation).map(|tool| tool.runs))
            .and_then(|runs| match runs {
                ToolRuns::Operation(index) => plan.operations.get(index.get()),
                ToolRuns::Pipeline(_) => None,
            })
            .is_some_and(|operation| {
                operation.examples.is_empty()
                    && !operation
                        .inputs
                        .iter()
                        .all(|input| !input.required || input.default.is_some())
            });
        order.push((needs_key, name, file));
    }
    order.sort_by_key(|(needs_key, _, _)| *needs_key);
    for (_, name, file) in order {
        let Ok(operation_name) = OperationName::new(name) else {
            continue;
        };
        let Some(tool) = plan.tool(&operation_name).cloned() else {
            tried.push(json!({"tool": name, "file": file, "state": "not_tried", "why": "The drafts hold no such tool."}));
            continue;
        };
        let found = FoundTool {
            target: target.clone(),
            plan_hash: compiled.encoded.hash,
            plan: Arc::clone(&plan),
            tool,
        };
        if found.effect().0 != a2m_core::EffectClass::Read {
            tried.push(json!({"tool": name, "file": file, "state": "not_tried", "why": "Only a read is tried; the scout never tries a write."}));
            continue;
        }
        let ToolRuns::Operation(index) = found.tool.runs else {
            tried.push(json!({"tool": name, "file": file, "state": "not_tried", "why": "Only an operation is tried; a pipeline is not."}));
            continue;
        };
        let Some(operation) = plan.operations.get(index.get()) else {
            continue;
        };
        let input = match operation.examples.first() {
            Some(example) => TryInput::Stated(example.clone()),
            None if operation
                .inputs
                .iter()
                .all(|input| !input.required || input.default.is_some()) =>
            {
                TryInput::Stated(Values::new())
            }
            None => key_from_list(operation, &proven),
        };
        let (input, from) = match input {
            TryInput::Stated(input) => (input, None),
            TryInput::FromList(input, list, output) => (input, Some((list, output))),
            TryInput::None(why) => {
                tried.push(json!({"tool": name, "file": file, "state": "not_tried", "why": why}));
                continue;
            }
        };
        let Some(environment) = environment.clone() else {
            tried.push(json!({"tool": name, "file": file, "state": "not_tried", "why": "The drafts declare no environment to try it in."}));
            continue;
        };
        let mut results: Vec<Json> = Vec::new();
        let mut kept = Vec::new();
        let mut not_ended = false;
        for try_number in 0..TRIES {
            // The call's wait is looked at between trials only, and after
            // one trial at least, so that every call moves the stage on.
            if waited && tokio::time::Instant::now() >= until {
                return Ok(Tried {
                    result: Json::Null,
                    runs,
                    complete: false,
                });
            }
            let (run, new) = trial_of(
                kernel,
                call,
                scout,
                &found,
                &input,
                try_number,
                &environment,
            )
            .await?;
            // A trial made before and found again does not count: the call
            // moves the stage on by one new trial at least.
            waited |= new;
            kept.push(run.to_string());
            let Some(row) = trial_rests(kernel, run).await? else {
                not_ended = true;
                break;
            };
            let shown = run_output(kernel, call, row, Tell::Nobody).await?;
            let completed = shown.result["status"] == "completed";
            results.push(shown.result);
            if !completed {
                // A trial that fails ends the read's tries: it is tried,
                // and not proven, with its named failure.
                break;
            }
        }
        runs.insert(name.clone(), kept);
        let completed = results
            .iter()
            .filter(|result| result["status"] == "completed")
            .count();
        let same = results
            .windows(2)
            .all(|pair| pair[0]["result"] == pair[1]["result"]);
        let state = if not_ended {
            "not_ended"
        } else if completed == TRIES && same {
            "proven"
        } else if completed == TRIES {
            "differs"
        } else {
            "failed"
        };
        let mut entry = json!({
            "tool": name,
            "file": file,
            "state": state,
            "tries": results.len(),
            "completed": completed,
            "same": same,
            "level": if state == "proven" { "executed" } else { "reconstructed" },
        });
        if not_ended {
            entry["why"] = json!(format!(
                "A trial did not end within {} seconds of its submission, and was cancelled.",
                TRIAL_BOUND.as_secs()
            ));
        }
        if let Some((list, output)) = &from {
            // Where the input came from, never its value.
            entry["input_from"] = json!({"list": list, "output": output});
        }
        if let Some(first) = results.first() {
            // The shape of what the system answered, never its values: a
            // list read may answer a thousand rows of the system's data.
            entry["shape"] = shape_of(&first["result"]);
        }
        if let Some(failure) = results.iter().find_map(|result| result.get("failure")) {
            // The class and code only: the message may quote the system.
            entry["failure"] = json!({"class": failure["class"], "code": failure["code"]});
        }
        if state == "proven"
            && let (Some(binding), Some(first)) = (operation.bindings.first(), results.first())
        {
            proven.push(Proven {
                tool: name.clone(),
                surface: binding.surface,
                lists: operation
                    .outputs
                    .iter()
                    .filter(|output| matches!(output.ty, a2m_core::ValueType::List(_)))
                    .map(|output| output.name.clone())
                    .collect(),
                result: first["result"].clone(),
            });
        }
        tried.push(entry);
    }
    // In the order of the drafted tools, whatever order they were tried in.
    tried.sort_by(|a, b| a["tool"].as_str().cmp(&b["tool"].as_str()));
    Ok(Tried {
        result: json!({"tools": tried, "left_out": left_out}),
        runs,
        complete: true,
    })
}

/// The problems of drafts as the scout reports them: a drafted file's
/// values, and an open question, may quote the system's own words, so each
/// problem's message is kept under `untrusted` (decisions 0171, 0180). The
/// code, the place, and the fix are the product's.
fn marked_problems(problems: &a2m_files::Diagnostics) -> Vec<Json> {
    problems_json(problems)
        .into_iter()
        .map(|mut problem| {
            if let Some(message) = problem.get_mut("message") {
                *message = untrusted(message.take());
            }
            problem
        })
        .collect()
}

/// The shape of a tried read's result, never its values (spec section
/// 218): each output's name and the kind of value it held, how many items
/// a list held, whether `more` was said, and a digest of the whole result,
/// so that two results can be told the same or not.
fn shape_of(result: &Json) -> Json {
    let kind = |value: &Json| match value {
        Json::Null => "null",
        Json::Bool(_) => "boolean",
        Json::Number(_) => "number",
        Json::String(_) => "text",
        Json::Array(_) => "list",
        Json::Object(_) => "record",
    };
    let outputs: Vec<Json> = result
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, value)| {
            let mut shown = json!({"output": name, "kind": kind(value)});
            if let Json::Array(items) = value {
                shown["items"] = json!(items.len());
                if let Some(first) = items.first() {
                    shown["item_kind"] = json!(kind(first));
                }
            }
            shown
        })
        .collect();
    let digest = a2m_core::CanonicalJson::encode(result)
        .map(|encoded| encoded.digest().to_string())
        .unwrap_or_default();
    json!({
        "outputs": outputs,
        "more": result.get("more").and_then(Json::as_bool),
        "digest": digest,
    })
}

/// What a scout says when it stops before its last stage.
fn stopped(
    target: &TargetName,
    scout: RunId,
    stage: u8,
    why: &str,
    done: &BTreeMap<u8, Json>,
) -> Output {
    Output::ok(json!({
        "target": target.as_str(),
        "scout_id": scout.to_string(),
        "state": "working",
        "stage": STAGES[usize::from(stage)],
        "stages_done": done.keys().map(|stage| STAGES[usize::from(*stage)]).collect::<Vec<_>>(),
        "why": why,
    }))
    .then(Next::new(
        "scout.run",
        json!({"target": target.as_str()}),
        "to continue from the last finished stage",
    ))
}

handler!(
    /// `scout.run`: the scout's fixed pipeline on one target, each stage
    /// stored before the next begins, so that a stopped scout continues
    /// from the last finished stage. Never writes to the system.
    scout_run,
    |kernel, call| {
        let target = TargetName::new(call.required("target")?)?;
        if kernel.settings().text("authoring.scout.run") == Some("off") {
            return Err(codes::policy(
                codes::SCOUT_OFF,
                "The setting `authoring.scout.run` is `off`: the scout does not run here.",
                "Run the stages one by one (`target.probe`, `discover.run`, `draft.write`), or ask a person to change the setting.",
            ));
        }
        let principal = &call.caller.principal;
        let (workspace, person) = (principal.workspace, principal.principal.id);
        let named = target.clone();
        let fresh = call.flag("fresh").unwrap_or(false);
        let project = project_key(&call);
        let in_project = project.clone();
        let scout = kernel
            .store()?
            .write(Durability::Synced, move |tx| {
                if !fresh
                    && let Some(scout) = newest_scout(tx, &workspace, &person, &named, &project, false)?
                {
                    return Ok(scout);
                }
                let id = RunId::from_parts(tx.now(), tx.random()?)?;
                let now = tx.now().as_unix_millis();
                tx.tables(Owner::Actions).execute(
                    "INSERT INTO actions_scouts (scout_id, workspace_id, principal_id, target, started_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    &[
                        SqlValue::from(id.to_string()),
                        SqlValue::from(workspace.to_string()),
                        SqlValue::from(person.to_string()),
                        SqlValue::from(named.as_str()),
                        SqlValue::from(now),
                    ],
                )?;
                Ok(Scout { id })
            })
            .await?;
        let id = scout.id;
        let mut done = kernel
            .store()?
            .read(move |tx| stored_stages(tx, id))
            .await?;
        let folder = folder_of(&call, &target).await?;

        // 0. Consent and budget.
        if let std::collections::btree_map::Entry::Vacant(slot) = done.entry(0) {
            // Consent is never recorded on a person's behalf (section
            // 213; audit finding A4): the engine keeps no record of who
            // added the target, and an agent cannot grant it. The stage
            // says what is true: who asked, and that no consent is held.
            let result = json!({
                "project": in_project,
                "consent": {
                    "recorded": false,
                    "asked_by": if call.caller.is_agent { "agent" } else { "person" },
                    "why": "No consent record exists for this target, and none is made for a person who did not give it. The scout reaches the system only through discovery and trials of reads, in the engine's sandbox, where nothing can be written.",
                },
                "decision": call.decision.to_string(),
                "bounds": "Each discoverer is held to the bounds its manifest declares, and each trial to the step limits of its tool.",
                "budget_in_force": false,
                "why_not": "The engine holds a discovery job to its discoverers' own bounds; no part of it keeps a budget for a whole scout yet.",
            });
            kernel
                .store_stage(id, 0, input_hash(&json!(target.as_str())), &result)
                .await?;
            slot.insert(result);
        }
        // 1. Identify; 2. discover. One discovery job per surface: the
        // engine runs every discoverer that suits a surface in one job.
        if !done.contains_key(&1) {
            let identified = match identify(kernel, &call, &target, None, true).await {
                Ok(identified) => identified,
                // Drafts that do not compile declare no surface the engine
                // can observe: the scout reports that and goes on, so that
                // a target just added gets a report and not an error.
                Err(failure) if failure.code() == &a2m_files::LINT_FAILED => Identified {
                    plan_hash: Digest::of(b"no plan"),
                    surfaces: Vec::new(),
                    runs: BTreeMap::new(),
                    complete: true,
                    not_compiled: Some(failure.message().to_owned()),
                },
                Err(failure) => return Err(failure),
            };
            if !identified.complete {
                return Ok(stopped(
                    &target,
                    id,
                    1,
                    "A discovery run has not ended yet.",
                    &done,
                ));
            }
            let declared = declared_sign_ins(kernel, &call, &target).await?;
            let mut result = json!({
                "plan_hash": identified.plan_hash.to_string(),
                "surfaces": identified.surfaces,
                "sign_in": sign_ins(&identified, declared.as_ref()),
                "runs": identified.runs_json(),
            });
            if let Some(why) = &identified.not_compiled {
                // The compiler's message may quote a drafted value, the
                // system's words: it is kept apart, under the mark.
                result["state"] = json!("not_looked_at");
                result["why"] = json!(
                    "The target's drafts do not compile, so no surface can be observed. `draft.lint` names every problem."
                );
                result["problems"] = untrusted(json!(why));
            }
            kernel
                .store_stage(id, 1, input_hash(&done[&0]), &result)
                .await?;
            done.insert(1, result);
        }
        let runs: BTreeMap<String, String> =
            serde_json::from_value(done[&1]["runs"].clone()).unwrap_or_default();
        let (findings, by_discoverer) = findings_of(kernel, workspace, &target, &runs).await?;
        if !done.contains_key(&2) {
            let found: Vec<Json> = findings
                .iter()
                .map(|finding| {
                    json!({
                        "id": finding.id().to_string(),
                        "kind": finding.kind().as_str(),
                        "level": finding.level().as_str(),
                        "source": finding.source().as_str(),
                        "coverage": finding.coverage().as_str(),
                        "limitations": finding.limitations().iter().map(|limit| limit.code.as_str()).collect::<Vec<_>>(),
                        "found": untrusted(json!(finding.found())),
                    })
                })
                .collect();
            let result = json!({"findings": found, "by_discoverer": by_discoverer});
            kernel
                .store_stage(id, 2, input_hash(&done[&1]), &result)
                .await?;
            done.insert(2, result);
        }
        // 3. Compare; 4. draft; 5. check. Only in a project that holds the
        // target's folder: drafts are files of the project.
        let no_folder = json!({"state": "not_looked_at", "why": "The call is not made in a project that holds the target's folder, so nothing is drafted."});
        if !done.contains_key(&3) {
            let result = match &folder {
                Some(_) => {
                    let (result, asked) =
                        compare(kernel, &call, &target, id, findings.clone()).await?;
                    let named = target.clone();
                    kernel
                        .store()?
                        .write(Durability::Synced, move |tx| {
                            open_questions(tx, workspace, &named, &asked)
                        })
                        .await?;
                    result
                }
                None => no_folder.clone(),
            };
            kernel
                .store_stage(id, 3, input_hash(&done[&2]), &result)
                .await?;
            done.insert(3, result);
        }
        if !done.contains_key(&4) {
            let result = match &folder {
                Some(folder) => {
                    let (result, asked) = draft(kernel, folder.clone(), findings.clone()).await?;
                    let named = target.clone();
                    kernel
                        .store()?
                        .write(Durability::Synced, move |tx| {
                            open_questions(tx, workspace, &named, &asked)
                        })
                        .await?;
                    result
                }
                None => no_folder.clone(),
            };
            kernel
                .store_stage(id, 4, input_hash(&done[&3]), &result)
                .await?;
            done.insert(4, result);
        }
        if !done.contains_key(&5) {
            let result = match &folder {
                Some(_) => {
                    let project = call.project_folder()?.to_path_buf();
                    match super::build::compile_in(
                        kernel,
                        project,
                        &target,
                        CompileMode::Draft,
                        DRAFT_VERSION,
                    )
                    .await?
                    {
                        Ok(compiled) => json!({
                            "compiles": true,
                            "problems": marked_problems(&compiled.warnings),
                            "tools": compiled.plan.tools.iter().map(|tool| tool.name.to_string()).collect::<Vec<_>>(),
                        }),
                        Err(problems) => {
                            json!({"compiles": false, "problems": marked_problems(&problems)})
                        }
                    }
                }
                None => no_folder.clone(),
            };
            kernel
                .store_stage(id, 5, input_hash(&done[&4]), &result)
                .await?;
            done.insert(5, result);
        }
        // 6. Try.
        if !done.contains_key(&6) {
            let result = if kernel.settings().flag("authoring.scout.try_reads") == Some(false) {
                json!({"state": "not_looked_at", "why": "The setting `authoring.scout.try_reads` is off."})
            } else if let Some(folder) = &folder {
                let written: Vec<String> = done[&4]["written"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|file| file["file"].as_str().map(str::to_owned))
                    .collect();
                let held_back: Vec<String> = done[&4]["held_back"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|file| file.as_str().map(str::to_owned))
                    .collect();
                let tools =
                    drafted_tools(kernel, folder.clone(), written, held_back.clone()).await?;
                let Tried {
                    mut result,
                    runs,
                    complete,
                } = try_reads(
                    kernel,
                    &call,
                    id,
                    &target,
                    folder.clone(),
                    &tools,
                    held_back,
                )
                .await?;
                if !complete {
                    return Ok(stopped(
                        &target,
                        id,
                        6,
                        "The call's wait ended before every trial was made; each made is kept, and the next call goes on from there.",
                        &done,
                    ));
                }
                result["runs"] = json!(runs);
                result
            } else {
                no_folder.clone()
            };
            kernel
                .store_stage(id, 6, input_hash(&done[&5]), &result)
                .await?;
            done.insert(6, result);
        }
        // 7. Report.
        if !done.contains_key(&7) {
            let named = target.clone();
            let open = kernel
                .store()?
                .read(move |tx| tx.findings().unknowns(&workspace, &named))
                .await?;
            let report = super::scout_report::assemble(&target, id, &done, &open);
            kernel
                .store_stage(id, 7, input_hash(&done[&6]), &report)
                .await?;
            done.insert(7, report);
        }
        let report = done.remove(&7).unwrap_or(Json::Null);
        Ok(super::scout_report::shown(report))
    }
);

handler!(
    /// `scout.report`: the last finished scout's report on a target, by
    /// the caller's person.
    scout_report,
    |kernel, call| {
        let target = TargetName::new(call.required("target")?)?;
        let principal = &call.caller.principal;
        let (workspace, person) = (principal.workspace, principal.principal.id);
        let named = target.clone();
        let project = project_key(&call);
        let found = kernel
            .store()?
            .read(move |tx| {
                let Some(scout) = newest_scout(tx, &workspace, &person, &named, &project, true)?
                else {
                    return Ok(None);
                };
                let unfinished = newest_scout(tx, &workspace, &person, &named, &project, false)?;
                Ok(Some((stored_stages(tx, scout.id)?, unfinished)))
            })
            .await?;
        let Some((mut stages, unfinished)) = found else {
            return Err(codes::input(
                codes::NOT_FOUND,
                format!("No scout of `{target}` has finished."),
                "Call `scout.run` on the target.",
            ));
        };
        let report = stages.remove(&7).unwrap_or(Json::Null);
        let mut output = super::scout_report::shown(report);
        if let Some(unfinished) = unfinished {
            output = output.warn(format!(
                "A newer scout, {}, has not finished; this is the last one that did.",
                unfinished.id
            ));
        }
        Ok(output)
    }
);

#[cfg(test)]
mod tests {
    use super::{COPY_MOST_FILES, copy_target};

    fn folder(name: &str) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("a2m-scout-copy-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(path.join("project").join("t").join("operations")).unwrap();
        path
    }

    /// Audit finding A5: a link in the target's folder is never followed,
    /// so a file outside the project is never copied into the cache.
    #[cfg(unix)]
    #[test]
    fn the_compare_copy_follows_no_link_out_of_the_project() {
        let root = folder("link");
        let project = root.join("project");
        std::fs::write(root.join("outside.yaml"), "secret-for-tests-outside").unwrap();
        std::fs::write(project.join("t").join("target.yaml"), "kept").unwrap();
        std::os::unix::fs::symlink(
            root.join("outside.yaml"),
            project.join("t").join("operations").join("swapped.yaml"),
        )
        .unwrap();
        let from = crate::handlers::build::project(&project)
            .unwrap()
            .join("t")
            .unwrap();
        let to = root.join("copy");
        copy_target(&from, &to, &mut (0, 0)).unwrap();
        assert_eq!(
            std::fs::read_to_string(to.join("target.yaml")).unwrap(),
            "kept"
        );
        assert!(!to.join("operations").join("swapped.yaml").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Audit finding A5, the race: a file the listing saw, swapped for a
    /// link before it is read, is refused by the read the copy makes of it
    /// (`std::fs::copy`, which the copy used before, follows the link).
    #[cfg(unix)]
    #[test]
    fn a_link_swapped_in_after_the_listing_is_not_read() {
        let root = folder("swap");
        let project = root.join("project");
        std::fs::write(root.join("outside.yaml"), "secret-for-tests-outside").unwrap();
        let file = project.join("t").join("operations").join("a.yaml");
        std::fs::write(&file, "kept").unwrap();
        let from = crate::handlers::build::project(&project)
            .unwrap()
            .join("t/operations")
            .unwrap();
        let (entries, _) = from.list().unwrap();
        std::fs::remove_file(&file).unwrap();
        std::os::unix::fs::symlink(root.join("outside.yaml"), &file).unwrap();
        assert!(entries[0].read(1024).is_err());
        let followed = std::fs::read_to_string(&file).unwrap();
        assert_eq!(followed, "secret-for-tests-outside");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Audit finding A5: the copy is bounded; a folder past the bound is
    /// refused, not copied whole.
    #[test]
    fn the_compare_copy_is_bounded() {
        let root = folder("bound");
        let operations = root.join("project").join("t").join("operations");
        for number in 0..=COPY_MOST_FILES {
            std::fs::write(operations.join(format!("o{number}.yaml")), "x").unwrap();
        }
        let from = crate::handlers::build::project(&root.join("project"))
            .unwrap()
            .join("t")
            .unwrap();
        let refused = copy_target(&from, &root.join("copy"), &mut (0, 0)).unwrap_err();
        assert_eq!(refused.code().as_str(), "path_not_allowed");
        let _ = std::fs::remove_dir_all(&root);
    }
}
