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
use super::tools::{Tell, plan_to_observe, run_output, submit_discovery, submit_trial, wait_of};
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
    target: TargetName,
    plan_hash: Digest,
    /// One entry per surface, with no run identifier in it.
    surfaces: Vec<Json>,
    /// The discovery run of each surface that has one.
    runs: BTreeMap<String, RunId>,
    /// Every discovery ended.
    complete: bool,
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
    let wait = wait_of(call);
    let workspace = call.caller.principal.workspace;
    let mut surfaces = Vec::new();
    let mut complete = true;
    for look in looks {
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
                entry["why"] = json!(
                    "No enabled discoverer suits this surface; it is not guessed about."
                );
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
        target: target.clone(),
        plan_hash,
        surfaces,
        runs,
        complete,
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
        let identified = identify(kernel, &call, &target, call.text("surface")).await?;
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
        let identified = identify(kernel, &call, &target, call.text("surface")).await?;
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
    finished: bool,
}

/// The newest scout of a target by the caller's person: the unfinished one
/// to resume, or with `finished`, the newest that finished.
fn newest_scout(
    tx: &mut Tx<'_>,
    workspace: &a2m_core::WorkspaceId,
    principal: &a2m_core::PrincipalId,
    target: &TargetName,
    finished: bool,
) -> Result<Option<Scout>, Failure> {
    let rows = tx.tables(Owner::Actions).query(
        "SELECT scout_id, finished_at FROM actions_scouts
         WHERE workspace_id = ?1 AND principal_id = ?2 AND target = ?3
         ORDER BY started_at DESC, scout_id DESC",
        &[
            SqlValue::from(workspace.to_string()),
            SqlValue::from(principal.to_string()),
            SqlValue::from(target.as_str()),
        ],
    )?;
    for row in rows {
        let done = row.opt_int(1)?.is_some();
        if done == finished {
            return Ok(Some(Scout {
                id: row.text(0)?.parse()?,
                finished: done,
            }));
        }
        if !finished {
            // The newest scout finished: there is none to resume.
            return Ok(None);
        }
    }
    Ok(None)
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

/// Copies the regular files of a target's folder, and its folders, into an
/// empty folder, never following a link and passing over hidden names.
fn copy_target(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let kind = std::fs::symlink_metadata(entry.path())?.file_type();
        if kind.is_dir() {
            copy_target(&entry.path(), &to.join(&name))?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), to.join(&name))?;
        }
    }
    Ok(())
}

/// Runs the drafter over `findings` in `folder`.
fn draft_into(kernel: &Kernel, folder: &Path, findings: &[Finding]) -> Result<DraftReport, Failure> {
    let set = kernel.providers().compiler_set();
    let root = TargetRoot::new(folder);
    Drafter::new(set, &root).draft(findings)
}

fn paths(list: &[a2m_files::RelPath]) -> Vec<String> {
    list.iter().map(|path| path.as_str().to_owned()).collect()
}

/// The compare stage: what the observations draft by rule, set against
/// what the target's files hold. The drafter is run over a copy of the
/// target's folder, so nothing in the project changes; a file it would
/// write where the target holds one a person wrote is a difference, and
/// each difference is an open question. A finding no rule covers is
/// listed, since nothing in the files can be compared with it by rule.
async fn compare(
    kernel: &Kernel,
    folder: PathBuf,
    scout: RunId,
    findings: Vec<Finding>,
) -> Result<(Json, Vec<(String, String, String, String)>), Failure> {
    let kernel = kernel.clone();
    off_thread(move || {
        let scratch = std::env::temp_dir().join(format!("a2m-scout-{scout}"));
        let _ = std::fs::remove_dir_all(&scratch);
        let copied = copy_target(&folder, &scratch);
        let drafted = match copied {
            Ok(()) => draft_into(&kernel, &scratch, &findings),
            Err(_) => Err(codes::input(
                codes::PATH_NOT_ALLOWED,
                "The target's folder could not be copied to compare it.",
                "Check that the target's folder may be read by this user.",
            )),
        };
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
            .map(|(path, problems)| (path.as_str().to_owned(), problems_json(problems)))
            .collect();
        let asked: Vec<(String, String, String, String)> = report
            .open_questions
            .iter()
            .map(|open| {
                (
                    open.question.clone(),
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
        Ok((
            json!({
                "written": files,
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
) -> Result<Vec<(String, String)>, Failure> {
    let kernel = kernel.clone();
    off_thread(move || {
        let set = kernel.providers().compiler_set();
        let Ok(sources) = a2m_files::load(&TargetRoot::new(&folder), set) else {
            return Ok(Vec::new());
        };
        let mut tools = Vec::new();
        for file in sources.of_kind("operation") {
            if !written.iter().any(|path| path == file.path.as_str()) {
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

/// The try stage: each drafted read tried in the sandbox, `TRIES` times,
/// one after another, and the results compared. A drafted read that takes
/// an input it states no example of is not tried: the scout has no value
/// for it and guesses none.
async fn try_reads(
    kernel: &Kernel,
    call: &Call,
    target: &TargetName,
    tools: &[(String, String)],
) -> Result<(Json, BTreeMap<String, Vec<String>>, bool), Failure> {
    let mut tried = Vec::new();
    let mut runs = BTreeMap::new();
    let mut complete = true;
    if tools.is_empty() {
        return Ok((json!({"tools": tried}), runs, complete));
    }
    let project = call.project_folder()?.to_path_buf();
    let compiled = super::build::compile_in(kernel, project, target, CompileMode::Draft, DRAFT_VERSION)
        .await?;
    let Ok(compiled) = compiled else {
        for (tool, file) in tools {
            tried.push(json!({"tool": tool, "file": file, "state": "not_tried", "why": "The drafts do not compile."}));
        }
        return Ok((json!({"tools": tried}), runs, complete));
    };
    let plan = Arc::new(compiled.plan);
    kernel.keep_draft(target, compiled.encoded.hash, Arc::clone(&plan));
    let environment = plan
        .default_environment()
        .map(|environment| environment.name.clone());
    for (name, file) in tools {
        let Ok(operation) = OperationName::new(name) else {
            continue;
        };
        let Some(tool) = plan.tool(&operation).cloned() else {
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
        let input = match &found.tool.runs {
            ToolRuns::Operation(index) => plan.operations.get(index.get()).and_then(|operation| {
                match operation.examples.first() {
                    Some(example) => Some(example.clone()),
                    None => operation
                        .inputs
                        .iter()
                        .all(|input| !input.required || input.default.is_some())
                        .then(Values::new),
                }
            }),
            ToolRuns::Pipeline(_) => None,
        };
        let (Some(input), Some(environment)) = (input, environment.clone()) else {
            tried.push(json!({"tool": name, "file": file, "state": "not_tried", "why": "It takes an input it states no example of; the scout has no value for it."}));
            continue;
        };
        let mut results = Vec::new();
        let mut kept = Vec::new();
        let mut ended = true;
        for _ in 0..TRIES {
            let run = submit_trial(kernel, call, &found, &input, environment.clone()).await?;
            kept.push(run.to_string());
            let row = kernel.wait_for_rest(run, wait_of(call)).await?;
            if !row.state.is_final() {
                ended = false;
                break;
            }
            let shown = run_output(kernel, call, row, Tell::Nobody).await?;
            results.push(shown.result);
        }
        complete &= ended;
        runs.insert(name.clone(), kept);
        let completed = results
            .iter()
            .filter(|result| result["status"] == "completed")
            .count();
        let same = results
            .windows(2)
            .all(|pair| pair[0]["result"] == pair[1]["result"]);
        let state = if !ended {
            "working"
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
        if let Some(first) = results.first() {
            entry["result"] = untrusted(first["result"].clone());
            if let Some(failure) = first.get("failure") {
                entry["failure"] = failure.clone();
            }
        }
        tried.push(entry);
    }
    Ok((json!({"tools": tried}), runs, complete))
}

/// What a scout says when it stops before its last stage.
fn stopped(target: &TargetName, scout: RunId, stage: u8, why: &str, done: &BTreeMap<u8, Json>) -> Output {
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
        let scout = kernel
            .store()?
            .write(Durability::Synced, move |tx| {
                if !fresh && let Some(scout) = newest_scout(tx, &workspace, &person, &named, false)? {
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
                Ok(Scout { id, finished: false })
            })
            .await?;
        let id = scout.id;
        let mut done = kernel.store()?.read(move |tx| stored_stages(tx, id)).await?;
        let folder = folder_of(&call, &target).await?;

        // 0. Consent and budget.
        if !done.contains_key(&0) {
            let result = json!({
                "consent": "The person who added the target, on this machine (section 402).",
                "decision": call.decision.to_string(),
                "bounds": "Each discoverer is held to the bounds its manifest declares, and each trial to the step limits of its tool.",
                "budget_in_force": false,
                "why_not": "The engine holds a discovery job to its discoverers' own bounds; no part of it keeps a budget for a whole scout yet.",
            });
            kernel.store_stage(id, 0, input_hash(&json!(target.as_str())), &result).await?;
            done.insert(0, result);
        }
        // 1. Identify; 2. discover. One discovery job per surface: the
        // engine runs every discoverer that suits a surface in one job.
        if !done.contains_key(&1) {
            let identified = identify(kernel, &call, &target, None).await?;
            if !identified.complete {
                return Ok(stopped(&target, id, 1, "A discovery run has not ended yet.", &done));
            }
            let declared = declared_sign_ins(kernel, &call, &target).await?;
            let result = json!({
                "plan_hash": identified.plan_hash.to_string(),
                "surfaces": identified.surfaces,
                "sign_in": sign_ins(&identified, declared.as_ref()),
                "runs": identified.runs_json(),
            });
            kernel.store_stage(id, 1, input_hash(&done[&0]), &result).await?;
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
            kernel.store_stage(id, 2, input_hash(&done[&1]), &result).await?;
            done.insert(2, result);
        }
        // 3. Compare; 4. draft; 5. check. Only in a project that holds the
        // target's folder: drafts are files of the project.
        let no_folder = json!({"state": "not_looked_at", "why": "The call is not made in a project that holds the target's folder, so nothing is drafted."});
        if !done.contains_key(&3) {
            let result = match &folder {
                Some(folder) => {
                    let (result, asked) = compare(kernel, folder.clone(), id, findings.clone()).await?;
                    let named = target.clone();
                    kernel
                        .store()?
                        .write(Durability::Synced, move |tx| open_questions(tx, workspace, &named, &asked))
                        .await?;
                    result
                }
                None => no_folder.clone(),
            };
            kernel.store_stage(id, 3, input_hash(&done[&2]), &result).await?;
            done.insert(3, result);
        }
        if !done.contains_key(&4) {
            let result = match &folder {
                Some(folder) => {
                    let (result, asked) = draft(kernel, folder.clone(), findings.clone()).await?;
                    let named = target.clone();
                    kernel
                        .store()?
                        .write(Durability::Synced, move |tx| open_questions(tx, workspace, &named, &asked))
                        .await?;
                    result
                }
                None => no_folder.clone(),
            };
            kernel.store_stage(id, 4, input_hash(&done[&3]), &result).await?;
            done.insert(4, result);
        }
        if !done.contains_key(&5) {
            let result = match &folder {
                Some(_) => {
                    let project = call.project_folder()?.to_path_buf();
                    match super::build::compile_in(kernel, project, &target, CompileMode::Draft, DRAFT_VERSION).await? {
                        Ok(compiled) => json!({
                            "compiles": true,
                            "problems": problems_json(&compiled.warnings),
                            "tools": compiled.plan.tools.iter().map(|tool| tool.name.to_string()).collect::<Vec<_>>(),
                        }),
                        Err(problems) => json!({"compiles": false, "problems": problems_json(&problems)}),
                    }
                }
                None => no_folder.clone(),
            };
            kernel.store_stage(id, 5, input_hash(&done[&4]), &result).await?;
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
                let tools = drafted_tools(kernel, folder.clone(), written).await?;
                let (mut result, runs, complete) = try_reads(kernel, &call, &target, &tools).await?;
                if !complete {
                    return Ok(stopped(&target, id, 6, "A trial has not ended yet.", &done));
                }
                result["runs"] = json!(runs);
                result
            } else {
                no_folder.clone()
            };
            kernel.store_stage(id, 6, input_hash(&done[&5]), &result).await?;
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
            kernel.store_stage(id, 7, input_hash(&done[&6]), &report).await?;
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
        let found = kernel
            .store()?
            .read(move |tx| {
                let Some(scout) = newest_scout(tx, &workspace, &person, &named, true)? else {
                    return Ok(None);
                };
                let unfinished = newest_scout(tx, &workspace, &person, &named, false)?;
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
