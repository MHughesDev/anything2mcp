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
    CompiledTarget, Digest, Environment, Failure, FailureClass, Finding, OperationName, RunId,
    SurfaceName, TargetName, ToolRuns, Values,
};
use a2m_files::{CompileMode, DRAFT_VERSION, DraftReport, Drafter, TargetRoot};
use a2m_ledger::{Durability, NewUnknown, Owner, SqlValue, Tx, UnknownStatus};
use a2m_provider::DiscoveryReport;
use serde_json::{Value as Json, json};

use super::build::{off_thread, problems_json, target_folder};
use super::handler;
use super::tools::{
    Tell, compiled_target_to_observe, discovery_files, files_by_surface, run_output,
    submit_discovery, surface_described, wait_of,
};
use crate::codes;
use crate::dispatch::{Call, ExitClass, Next, Output};
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
            "failure": {
                "class": failure.class().as_str(),
                "code": failure.code().as_str(),
                // The fix the failure's own table declares, never what a
                // system said.
                "fix": failure.next_step(),
            },
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
    access_kind: Option<&'static str>,
    provider: Option<String>,
    keeps_a_session: bool,
    discoverers: Vec<Json>,
    /// The provider of the sign-in the target's files declare for it, when
    /// they declare one: its discovery and its trials then run under the
    /// caller's person's session.
    signs_in_with: Option<String>,
}

impl Look {
    fn of(kernel: &Kernel, compiled_target: &CompiledTarget) -> Vec<Self> {
        let registry = kernel.providers();
        compiled_target.surfaces
            .iter()
            .enumerate()
            .map(|(position, surface)| {
                let entry = registry.surface_kind(&surface.kind);
                // Described with its address in the environment a discovery
                // runs in.
                let discoverers = surface_described(kernel, compiled_target, position)
                    .map(|described| {
                        registry
                            .discoverers_for(&described)
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
                    access_kind: entry.map(|entry| entry.access_kind.as_str()),
                    provider: entry.map(|entry| entry.provider.to_string()),
                    keeps_a_session: entry.is_some_and(|entry| entry.session),
                    discoverers,
                    signs_in_with: u32::try_from(position)
                        .ok()
                        .and_then(|at| compiled_target.auth_of(a2m_core::SurfaceIndex::new(at)))
                        .map(|auth| auth.provider.to_string()),
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

/// The rows of one discovery run, or of one capture, by its identifier:
/// a capture's discoverers report under the capture's own (decision 0433).
fn discovery_results(tx: &mut Tx<'_>, run: &str) -> Result<Vec<Reported>, Failure> {
    let rows = tx.tables(Owner::Actions).query(
        "SELECT provider, discoverer, findings, report FROM actions_discovery_results
         WHERE run_id = ?1 ORDER BY provider, discoverer",
        &[SqlValue::from(run)],
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
/// A discovery that could not be signed in, because nobody is signed in or
/// the credential its sign-in names is not stored, needs a person to sign
/// in (decision 0205).
fn identified_state(
    state: a2m_core::RunState,
    failure: Option<&(FailureClass, a2m_core::FailureCode)>,
) -> &'static str {
    if !state.is_final() {
        return "working";
    }
    match failure {
        None => "identified",
        Some((FailureClass::SessionExpired, _)) => "needs_login",
        Some((_, code)) if code == &a2m_auth::codes::CREDENTIAL_MISSING => "needs_login",
        Some((FailureClass::Policy | FailureClass::EdgeBlocked, _)) => "refused",
        Some(_) => "failed",
    }
}

/// What the identify stage came to.
struct Identified {
    compiled_target_hash: Digest,
    /// One entry per surface, with no run identifier in it.
    surfaces: Vec<Json>,
    /// The discovery run of each surface that has one.
    runs: BTreeMap<String, RunId>,
    /// Every discovery ended.
    complete: bool,
    /// Why nothing was observed, when the drafts do not compile.
    not_compiled: Option<String>,
    /// The `sign_in_method` findings of each surface's discovery, for the
    /// rules that recommend a sign-in.
    sign_in_findings: BTreeMap<String, Vec<Finding>>,
}

/// The files of the project's `inputs` folder a scout hands its
/// discoveries (decision 0263), and whether they are an unfinished scout's,
/// stored when it began, rather than the call's.
#[derive(Debug, Default)]
struct Handed {
    files: BTreeMap<String, String>,
    resumed: bool,
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
    files: &Handed,
) -> Result<Identified, Failure> {
    let (compiled_target_hash, compiled_target) =
        compiled_target_to_observe(kernel, call, target).await?;
    let looks: Vec<Look> = Look::of(kernel, &compiled_target)
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
    let environment: Option<Environment> = compiled_target
        .default_environment()
        .map(|environment| environment.name.clone());
    // The files the call hands are split by surface: each discovery is
    // handed those a discoverer suiting its surface reads, and a name none
    // reads starts nothing (decision 0263).
    let observed: Vec<SurfaceName> = looks
        .iter()
        .filter(|look| look.provider.is_some() && !look.discoverers.is_empty())
        .map(|look| look.name.clone())
        .collect();
    let handed = files_by_surface(
        kernel,
        &compiled_target,
        &observed,
        &files.files,
        files.resumed,
    )?;
    let handed: BTreeMap<&SurfaceName, &BTreeMap<String, String>> =
        observed.iter().zip(&handed).collect();
    let none = BTreeMap::new();
    let mut runs = BTreeMap::new();
    for look in &looks {
        if look.provider.is_some()
            && !look.discoverers.is_empty()
            && let Some(environment) = &environment
        {
            let files = handed.get(&look.name).copied().unwrap_or(&none);
            let run = submit_discovery(
                kernel,
                call,
                target,
                compiled_target_hash,
                environment,
                &look.name,
                files,
            )
            .await?;
            runs.insert(look.name.to_string(), run);
        }
    }
    let workspace = call.caller.principal.workspace;
    let mut surfaces = Vec::new();
    let mut sign_in_findings: BTreeMap<String, Vec<Finding>> = BTreeMap::new();
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
            "access_kind": look.access_kind,
            "provider": look.provider,
            "keeps_a_session": look.keeps_a_session,
            "discoverers": look.discoverers,
            // Whether what is found here needed the session: a surface whose
            // sign-in the files declare is observed only under the caller's
            // person's session, and nothing of it is found without one.
            "session": match &look.signs_in_with {
                Some(provider) => json!({"needed": true, "provider": provider}),
                None => json!({"needed": false}),
            },
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
                    .read(move |tx| discovery_results(tx, &run.to_string()))
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
                sign_in_findings.insert(
                    look.name.to_string(),
                    found
                        .iter()
                        .filter(|finding| finding.kind().as_str() == a2m_auth::SIGN_IN_METHOD)
                        .cloned()
                        .collect(),
                );
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
                identified_state(row.state, row.failure.as_ref())
            }
        };
        entry["state"] = json!(state);
        surfaces.push(entry);
    }
    Ok(Identified {
        compiled_target_hash,
        surfaces,
        runs,
        complete,
        not_compiled: None,
        sign_in_findings,
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
        let identified = identify(
            kernel,
            &call,
            &target,
            call.text("surface"),
            false,
            &Handed::default(),
        )
        .await?;
        let output = Output::ok(json!({
            "target": target.as_str(),
            "compiled_target_hash": identified.compiled_target_hash.to_string(),
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
/// `auth.yaml` writes them: the identity kind and the provider. `None` when
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
            // The fixed rules of `a2m-auth` over what this surface's
            // discovery found. Always marked inferred; what the files
            // declare is kept as declared, and the recommendation stands
            // beside it (decision 0367).
            let recommendation = a2m_auth::recommend(
                identified
                    .sign_in_findings
                    .get(name)
                    .map_or(&[][..], Vec::as_slice),
            );
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
                "level": if first.is_some() {
                    "declared"
                } else if recommendation.first().is_some() {
                    "inferred"
                } else {
                    "unknown"
                },
                "unknown": unknown,
                "recommendation": recommendation.to_json(),
                "confirm": "A claim for a person to confirm by signing in; the engine never holds a credential it was told here.",
            })
        })
        .collect()
}

handler!(
    /// `auth.detect`: how each surface signs in, as claims to be confirmed
    /// by a person: whether its kind keeps a session, what the target's
    /// files declare, and what its discovery met. The fixed rules of
    /// `a2m-auth` recommend a mode and a provider from what was met,
    /// marked `inferred`, for a person to confirm; what the files declare
    /// is kept as declared, and a sign no rule covers is a question.
    auth_detect,
    |kernel, call| {
        let target = TargetName::new(call.required("target")?)?;
        let identified = identify(
            kernel,
            &call,
            &target,
            call.text("surface"),
            false,
            &Handed::default(),
        )
        .await?;
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
    /// When it finished, by the store's clock; `None` while it goes on.
    finished_at: Option<i64>,
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
/// The project a scout was made in is stored in its row, with the row
/// (migration 0018); a scout with none stored is of no project, and is
/// never resumed.
fn newest_scout(
    tx: &mut Tx<'_>,
    workspace: &a2m_core::WorkspaceId,
    principal: &a2m_core::PrincipalId,
    target: &TargetName,
    project: &str,
    finished: bool,
) -> Result<Option<Scout>, Failure> {
    let rows = tx.tables(Owner::Actions).query(
        "SELECT s.scout_id, s.finished_at FROM actions_scouts s
         WHERE s.workspace_id = ?1 AND s.principal_id = ?2 AND s.target = ?3
             AND s.project = ?4
         ORDER BY s.started_at DESC, s.scout_id DESC",
        &[
            SqlValue::from(workspace.to_string()),
            SqlValue::from(principal.to_string()),
            SqlValue::from(target.as_str()),
            SqlValue::from(project),
        ],
    )?;
    for row in rows {
        let finished_at = row.opt_int(1)?;
        if finished_at.is_some() == finished {
            return Ok(Some(Scout {
                id: row.text(0)?.parse()?,
                finished_at,
            }));
        }
        if !finished {
            // The newest scout finished: there is none to resume.
            return Ok(None);
        }
    }
    Ok(None)
}

/// The scout `id` of a target by the caller's person in one project, when
/// there is one.
fn named_scout(
    tx: &mut Tx<'_>,
    workspace: &a2m_core::WorkspaceId,
    principal: &a2m_core::PrincipalId,
    target: &TargetName,
    project: &str,
    id: RunId,
) -> Result<Option<Scout>, Failure> {
    let rows = tx.tables(Owner::Actions).query(
        "SELECT s.scout_id, s.finished_at FROM actions_scouts s
         WHERE s.scout_id = ?1 AND s.workspace_id = ?2 AND s.principal_id = ?3
             AND s.target = ?4 AND s.project = ?5",
        &[
            SqlValue::from(id.to_string()),
            SqlValue::from(workspace.to_string()),
            SqlValue::from(principal.to_string()),
            SqlValue::from(target.as_str()),
            SqlValue::from(project),
        ],
    )?;
    match rows.first() {
        Some(row) => Ok(Some(Scout {
            id: row.text(0)?.parse()?,
            finished_at: row.opt_int(1)?,
        })),
        None => Ok(None),
    }
}

fn no_such_scout(target: &TargetName) -> Failure {
    codes::input(
        codes::NOT_FOUND,
        format!("No such scout of `{target}` has finished."),
        "Call `scout.run` on the target, or `capture.stop` on the capture the report was made from.",
    )
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
    let Some(tried) = scout_tried(kernel, call, target).await? else {
        return Ok(None);
    };
    Ok(tried
        .entries
        .iter()
        .find(|entry| {
            entry["tool"] == tool && entry["state"] == "proven" && entry["level"] == "executed"
        })
        .map(|entry| {
            json!({
                "by": "scout",
                "scout_id": tried.scout.to_string(),
                "level": "executed",
                "tries": entry["tries"],
                "binding": entry["binding"],
            })
        }))
}

/// What the newest finished scout of a target tried, as its try stage
/// stored it, already redacted.
pub(crate) struct ScoutTried {
    /// The scout.
    pub(crate) scout: RunId,
    /// When it finished, by the store's clock.
    pub(crate) finished_at: a2m_core::Timestamp,
    /// One entry for each drafted read: its `tool`, `state`, `binding`,
    /// and, for a proven read, the `call` that reads it.
    pub(crate) entries: Vec<Json>,
}

/// The try stage of the newest finished scout of the target by the
/// caller's person, in the call's project: `None` when there is none.
pub(crate) async fn scout_tried(
    kernel: &Kernel,
    call: &Call,
    target: &TargetName,
) -> Result<Option<ScoutTried>, Failure> {
    let principal = &call.caller.principal;
    let (workspace, person) = (principal.workspace, principal.principal.id);
    let (named, project) = (target.clone(), project_key(call));
    let stages = kernel
        .store()?
        .read(move |tx| {
            let Some(scout) = newest_scout(tx, &workspace, &person, &named, &project, true)? else {
                return Ok(None);
            };
            let stages = stored_stages(tx, scout.id)?;
            Ok(Some((scout, stages)))
        })
        .await?;
    let Some((scout, stages)) = stages else {
        return Ok(None);
    };
    let entries = stages
        .get(&6)
        .and_then(|tried| tried["tools"].as_array())
        .cloned()
        .unwrap_or_default();
    Ok(Some(ScoutTried {
        scout: scout.id,
        finished_at: a2m_core::Timestamp::from_unix_millis(scout.finished_at.unwrap_or(0)),
        entries,
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

/// The files a scout began with, as its first stage stores them: `None`
/// when that stage is not stored yet; `Some(None)` when what is stored
/// cannot be read as files; none when the stage names none, as a scout
/// stored before files could be handed (decision 0263).
fn stored_files(
    tx: &mut Tx<'_>,
    scout: RunId,
) -> Result<Option<Option<BTreeMap<String, String>>>, Failure> {
    let stages = stored_stages(tx, scout)?;
    Ok(stages.get(&0).map(|first| match first.get("files") {
        None | Some(Json::Null) => Some(BTreeMap::new()),
        Some(files) => serde_json::from_value(files.clone()).ok(),
    }))
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

/// The findings an identify stage's runs made, in a fixed order. A run
/// here is a discovery run, or the capture a scout from a capture began
/// with.
async fn findings_of(
    kernel: &Kernel,
    workspace: a2m_core::WorkspaceId,
    target: &TargetName,
    runs: &BTreeMap<String, String>,
) -> Result<(Vec<Finding>, Vec<Json>), Failure> {
    let runs: Vec<(String, String)> = runs
        .iter()
        .map(|(surface, run)| (surface.clone(), run.clone()))
        .collect();
    let target = target.clone();
    kernel
        .store()?
        .read(move |tx| {
            let mut findings = BTreeMap::<Digest, Finding>::new();
            let mut by_discoverer = Vec::new();
            for (surface, run) in &runs {
                for reported in discovery_results(tx, run)? {
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

/// A draft's path without the holding folder's name in front. An unknown is
/// kept under its scope, which is this path, so that the same draft held or
/// not is one question and moving it in or out opens none again.
fn unheld(path: &a2m_files::RelPath) -> String {
    path.as_str()
        .strip_prefix(&format!("{}/", a2m_files::HELD_FOLDER))
        .unwrap_or(path.as_str())
        .to_owned()
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
            let path = unheld(path);
            asked.push((
                format!(
                    "The system, as observed, drafts `{path}` otherwise than the target's file holds it. Which is right?"
                ),
                path.clone(),
                "The file is the target's, and what was observed differs from it.".to_owned(),
                "A person reads the file beside the findings it would be drafted from (`finding.describe`).".to_owned(),
            ));
        }
        Ok((
            json!({
                "differs": paths(&report.conflicts),
                "would_draft": paths(&[report.written.clone(), report.held.clone()].concat()),
                "not_covered": report.not_covered.iter().map(Digest::to_string).collect::<Vec<_>>(),
            }),
            asked,
        ))
    })
    .await
}

/// The key of the stage-3 result that holds the comparison of what is
/// described with what is observed (decision 0486).
pub(crate) const DESCRIBED_AGAINST_OBSERVED: &str = "described_against_observed";

/// Described against observed (decision 0486): the registry's comparison
/// of the findings, as stage 3 keeps it, and a question for each entry
/// that is not `matched`. A verb and a path are the system's words, kept
/// under the mark; a question names neither, only the finding it is
/// scoped to.
fn described_against_observed(
    kernel: &Kernel,
    findings: &[Finding],
) -> (Json, Vec<(String, String, String, String)>) {
    use a2m_core::{ComparisonResult, NotCompared};
    let comparison = kernel.providers().compare(findings);
    let ids = |list: &[Digest]| list.iter().map(Digest::to_string).collect::<Vec<_>>();
    let mut asked = Vec::new();
    let entries: Vec<Json> = comparison
        .entries
        .iter()
        .map(|entry| {
            let scope = entry
                .described
                .first()
                .or(entry.observed.first())
                .map(Digest::to_string)
                .unwrap_or_default();
            let question = match entry.result {
                ComparisonResult::Matched => None,
                ComparisonResult::DescribedOnly => Some((
                    "The system's description states this endpoint, and this observation did not see it. Does it work as described?".to_owned(),
                    "A tool drafted from it rests on the description alone, at `inferred`.".to_owned(),
                    "An observation of the surface that reaches it, such as a capture of a person's use, at `discovered`.".to_owned(),
                )),
                ComparisonResult::ObservedOnly => Some((
                    "This endpoint was seen, and no description states it. What is it?".to_owned(),
                    "Nothing but the observation says what it takes and what it returns.".to_owned(),
                    "A description that states it, or a person's account in the target's files.".to_owned(),
                )),
                ComparisonResult::Unknown => Some((
                    format!(
                        "What the description states here could not be compared with an observation ({}). Is it there?",
                        entry.reason.map_or("unknown", NotCompared::as_str)
                    ),
                    "Whether it was seen or not is not known, so nothing rests on it but the description.".to_owned(),
                    "An observation of the place it names, with its coverage stated.".to_owned(),
                )),
            };
            if let Some((question, why, settled_by)) = question {
                asked.push((question, scope, why, settled_by));
            }
            json!({
                "result": entry.result.as_str(),
                "level": entry.level.as_str(),
                "coverage": entry.coverage.as_str(),
                "reason": entry.reason.map(NotCompared::as_str),
                "described": ids(&entry.described),
                "observed": ids(&entry.observed),
                "endpoint": entry.key.as_ref().map(|key| untrusted(json!({
                    "place": key.place(),
                    "verb": key.verb(),
                    "path": key.path(),
                }))),
            })
        })
        .collect();
    let counts: serde_json::Map<String, Json> = ComparisonResult::ALL
        .iter()
        .map(|result| (result.as_str().to_owned(), json!(comparison.count(*result))))
        .collect();
    (json!({"counts": counts, "entries": entries}), asked)
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
                    unheld(&open.file),
                    format!("The draft `{}` waits on the answer.", unheld(&open.file)),
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
        // The drafts that wait in the holding folder, each once: every
        // file of a finding that has an open question. The loader does not
        // read them, so the target compiles and is tried without them
        // (decision 0298).
        let held_back: BTreeSet<String> = report
            .held
            .iter()
            .map(|path| path.as_str().to_owned())
            .collect();
        // The findings a rule covers and drafted nothing from, each once,
        // with the finding's own words (looked up by its id) and the
        // reasons its rules gave (roadmap package FT11). Two rules of one
        // pack that cover the same kind each leave the finding; it is
        // listed once, with each reason once (package 5.6c).
        let mut not_drafted: Vec<Json> = Vec::new();
        for left in &report.not_drafted {
            let id = left.finding.to_string();
            let reason = left.reason.trim();
            if let Some(listed) = not_drafted
                .iter_mut()
                .find(|listed| listed["finding"] == id.as_str() && listed["pack"] == left.pack.as_str())
            {
                if let Some(reasons) = listed["reasons"].as_array_mut()
                    && !reason.is_empty()
                    && !reasons.iter().any(|known| known == reason)
                {
                    reasons.push(json!(reason));
                }
                continue;
            }
            let found = findings.iter().find(|finding| finding.id() == left.finding);
            let description = found.map_or(Json::Null, |finding| untrusted(json!(finding.found())));
            let kind = found.map(|finding| finding.kind().as_str().to_owned());
            not_drafted.push(json!({
                "finding": id,
                "kind": kind,
                "description": description,
                "pack": left.pack,
                "reasons": if reason.is_empty() { json!([]) } else { json!([reason]) },
            }));
        }
        // The reason the entry is read by: the rules' own, joined, or,
        // where no rule gave one, what the report knows of it.
        for listed in &mut not_drafted {
            let reasons: Vec<String> = listed["reasons"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Json::as_str)
                .map(str::to_owned)
                .collect();
            listed["reason"] = json!(if reasons.is_empty() {
                format!(
                    "No rule of the `{}` pack{} drafts a file from one of this shape, and none said why. No draft waits on it: it is listed under `found`, and a draft over it is written by hand.",
                    listed["pack"].as_str().unwrap_or_default(),
                    listed["kind"]
                        .as_str()
                        .map(|kind| format!(" for `{kind}` findings"))
                        .unwrap_or_default()
                )
            } else {
                reasons.join(" ")
            });
            if let Json::Object(map) = listed {
                map.remove("reasons");
            }
        }
        Ok((
            json!({
                "written": files,
                "held_back": held_back,
                "not_drafted": not_drafted,
                "left_out_as_secret": report.left_out_as_secret,
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
/// compiled target tried, the read, which try it is, and the input. A
/// continuation of the scout finds the same trial by it, ended or not, and
/// submits none again (ledger rule 6).
fn trial_hash(
    scout: RunId,
    compiled_target: Digest,
    try_number: usize,
    input: &Values,
    binding: &a2m_engine::BindingPin,
) -> Digest {
    let mut hasher = a2m_core::FieldHasher::new();
    hasher.field("purpose", b"anything2mcp scout trial 2");
    hasher.field("scout", scout.to_string().as_bytes());
    hasher.field("plan", compiled_target.to_string().as_bytes());
    hasher.field("try", try_number.to_string().as_bytes());
    hasher.field("input", input.digest().to_string().as_bytes());
    hasher.field("binding", binding.to_json().to_string().as_bytes());
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
    (try_number, environment, binding): (usize, &Environment, &a2m_engine::BindingPin),
) -> Result<(RunId, bool), Failure> {
    let hash = trial_hash(
        scout,
        found.compiled_target_hash,
        try_number,
        input,
        binding,
    );
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
        None => super::tools::submit_trial_as(
            kernel,
            call,
            found,
            input,
            environment.clone(),
            hash,
            Some(binding.clone()),
        )
        .await
        .map(|run| (run, true)),
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
/// output of exactly the key's name whose type is a list, or, when none
/// does, of exactly that name without its `_id` (decision 0445); the first
/// item of that output, in the list's own order, is text or a whole number.
/// Anything else is not tried, with its reason (rule 7: no guessing by
/// resemblance).
fn key_from_list(operation: &a2m_core::CompiledOperation, proven: &[Proven]) -> TryInput {
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
    let Some(surface) = a2m_engine::default_binding(operation)
        .and_then(|at| operation.bindings.get(at))
        .map(|binding| binding.surface)
    else {
        return TryInput::None("It has no binding to try.");
    };
    // The output of the key's own name; when no list answers one, the
    // output of that name without its `_id`, as a key `order_id` is read
    // off a list's `order` column (package 5.6b). Exactly one list read
    // must answer it, under either rule.
    let answering = |name: &str| -> Vec<&Proven> {
        proven
            .iter()
            .filter(|read| {
                read.surface == surface
                    && read.lists.iter().any(|listed| listed == name)
                    && read.result.get(name).is_some_and(Json::is_array)
            })
            .collect()
    };
    let mut output = key.name.clone();
    let mut lists = answering(&output);
    if lists.is_empty()
        && let Some(record) = key
            .name
            .strip_suffix("_id")
            .filter(|record| !record.is_empty())
    {
        output = record.to_owned();
        lists = answering(&output);
    }
    let [
        Proven {
            tool: list, result, ..
        },
    ] = lists.as_slice()
    else {
        return TryInput::None(if lists.is_empty() {
            "It takes a key it states no example of, and no list read of the same surface proven in this scout answers an output of the key's name, or of that name without its `_id`."
        } else {
            "It takes a key it states no example of, and more than one list read proven in this scout answers an output of the key's name; nothing says which is of the same record."
        });
    };
    let value = match result[&output].get(0) {
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
    TryInput::FromList(Values::new().with(&key.name, value), list.clone(), output)
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
            result: json!({"tools": tried, "left_out": left_out, "counts": tried_counts(&tried, &left_out)}),
            runs,
            complete: true,
        });
    }
    // What can compile is compiled: the target as the loader reads it. A
    // draft that holds an open question waits in the holding folder, which
    // is not read (decision 0298), so it is not among the files.
    let compiled = {
        let kernel = kernel.clone();
        off_thread(move || Ok(super::build::compile_what_can(&kernel, &folder, &held_back))).await?
    };
    let (compiled, left_out) = match compiled {
        Ok((compiled, left_out)) => (compiled, json!(left_out)),
        Err(problems) => {
            for (tool, file) in tools {
                tried.push(json!({"tool": tool, "file": file, "state": "not_tried", "why": "The drafts do not compile."}));
            }
            return Ok(Tried {
                result: json!({"tools": tried, "left_out": left_out, "counts": tried_counts(&tried, &left_out), "problems": marked_problems(&problems)}),
                runs,
                complete: true,
            });
        }
    };
    let compiled_target = Arc::new(compiled.compiled_target);
    // A scout's trial lets no asserted read through: it is not a try a
    // person asked for (decision 0475).
    kernel.keep_draft(
        target,
        compiled.encoded.hash,
        Arc::clone(&compiled_target),
        a2m_provider::ConfirmedReads::none(),
    );
    let environment = compiled_target
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
            .and_then(|operation| compiled_target.tool(&operation).map(|tool| tool.runs))
            .and_then(|runs| match runs {
                ToolRuns::Operation(index) => compiled_target.operations.get(index.get()),
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
        let Some(tool) = compiled_target.tool(&operation_name).cloned() else {
            tried.push(json!({"tool": name, "file": file, "state": "not_tried", "why": "The drafts hold no such tool."}));
            continue;
        };
        let found = FoundTool {
            target: target.clone(),
            compiled_target_hash: compiled.encoded.hash,
            compiled_target: Arc::clone(&compiled_target),
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
        let Some(operation) = compiled_target.operations.get(index.get()) else {
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
        // The scout tries the binding that would act live, and its proof
        // names that binding and the digest of what it does (audit finding
        // AC-6).
        let Some(binding) = a2m_engine::default_binding(operation)
            .and_then(|at| a2m_engine::BindingPin::of(&compiled_target, operation, at))
        else {
            tried.push(json!({"tool": name, "file": file, "state": "not_tried", "why": "Every binding of it is retired."}));
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
                (try_number, &environment, &binding),
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
        let state = trial_state(&results, not_ended);
        let waits = state == "waiting";
        let mut entry = json!({
            "tool": name,
            "file": file,
            "state": state,
            "tries": results.len(),
            "completed": completed,
            "same": same,
            "level": if state == "proven" { "executed" } else { "reconstructed" },
            "binding": binding.to_json(),
        });
        if not_ended {
            entry["why"] = json!(format!(
                "A trial did not end within {} seconds of its submission, and was cancelled.",
                TRIAL_BOUND.as_secs()
            ));
        }
        if waits
            && let Some(surface) =
                super::tools::binding_surface(&compiled_target, &found.tool, Some(&binding))
        {
            let next = super::tools::sign_in_next(target, &surface);
            entry["surface"] = json!(surface.as_str());
            entry["next"] = json!({"action": next.action, "input": next.args});
        }
        if let Some((list, output)) = &from {
            // Where the input came from, never its value.
            entry["input_from"] = json!({"list": list, "output": output});
        }
        if state == "proven" {
            // The call that reads it, for an agent to copy (decision 0306).
            // An input taken from a list read's answer is the system's
            // data: it is not copied into the report, and the call names
            // where it comes from.
            let mut arguments = json!({"target": target.as_str(), "tool": name});
            if from.is_none()
                && !input.is_empty()
                && let Ok(accepted) = serde_json::to_value(&input)
            {
                arguments["input"] = accepted;
            }
            entry["call"] = json!({"action": "draft.try", "input": arguments});
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
            && let (Some(acted), Some(first)) =
                (operation.bindings.get(binding.position), results.first())
        {
            proven.push(Proven {
                tool: name.clone(),
                surface: acted.surface,
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
    let counts = tried_counts(&tried, &left_out);
    Ok(Tried {
        result: json!({"tools": tried, "left_out": left_out, "counts": counts}),
        runs,
        complete: true,
    })
}

/// What the trials of one drafted read came to. A read whose last trial
/// rests waiting for a sign-in is not a failed read: it waits, with the
/// surface to sign in to (decision 0303).
fn trial_state(results: &[Json], not_ended: bool) -> &'static str {
    let completed = results
        .iter()
        .filter(|result| result["status"] == "completed")
        .count();
    let same = results
        .windows(2)
        .all(|pair| pair[0]["result"] == pair[1]["result"]);
    let waits = results
        .last()
        .is_some_and(|result| result["status"] == "needs_login");
    if not_ended {
        "not_ended"
    } else if waits {
        "waiting"
    } else if completed == TRIES && same {
        "proven"
    } else if completed == TRIES {
        "differs"
    } else {
        "failed"
    }
}

/// What became of the drafted reads, counted so that they add up (decision
/// 0303): every drafted read is proven, waiting for a sign-in, failed (a
/// trial that failed, differed, or did not end), not tried (with its
/// reason in its entry), or held back with an open question.
fn tried_counts(tried: &[Json], left_out: &Json) -> Json {
    let state = |name: &str| tried.iter().filter(|tool| tool["state"] == name).count();
    let (proven, waiting, not_tried) = (state("proven"), state("waiting"), state("not_tried"));
    let failed = tried.len() - proven - waiting - not_tried;
    let held = left_out.as_array().map_or(0, Vec::len);
    json!({
        "drafted": tried.len() + held,
        "proven": proven,
        "waiting": waiting,
        "failed": failed,
        "not_tried": not_tried,
        "held": held,
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

/// The scout stops, before it drafts, at a surface its person must sign in
/// to first (decision 0205): `session.login` comes next, then the same scout
/// again.
fn needs_sign_in(
    target: &TargetName,
    scout: RunId,
    surface: &Json,
    done: &BTreeMap<u8, Json>,
) -> Output {
    let name = surface["surface"].as_str().unwrap_or_default();
    Output::ok(json!({
        "target": target.as_str(),
        "scout_id": scout.to_string(),
        "state": "needs_sign_in",
        "stage": STAGES[1],
        "stages_done": done.keys().map(|stage| STAGES[usize::from(*stage)]).collect::<Vec<_>>(),
        "surface": name,
        "failure": surface.get("failure"),
        "why": format!(
            "The surface `{name}` is observed only under a session, and its person is not signed in to it. Nothing was drafted."
        ),
    }))
    .exit(ExitClass::PersonNeeded)
    .then(Next::new(
        "session.login",
        json!({"target": target.as_str(), "surface": name}),
        "the person signs in; then call `scout.run` again, and the same scout goes on under the session",
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
        // The files the call hands the scout's discoveries are checked
        // before anything is stored or started: each path, and that a
        // discoverer suiting an observed surface reads each name (decision
        // 0263).
        let asked = discovery_files(kernel, &call, call.input.get("files")).await?;
        if !asked.is_empty() {
            let (_, compiled_target) = compiled_target_to_observe(kernel, &call, &target).await?;
            let observed: Vec<SurfaceName> = Look::of(kernel, &compiled_target)
                .into_iter()
                .filter(|look| look.provider.is_some() && !look.discoverers.is_empty())
                .map(|look| look.name)
                .collect();
            files_by_surface(kernel, &compiled_target, &observed, &asked, false)?;
        }
        let project = project_key(&call);
        let in_project = project.clone();
        let asking = asked.clone();
        let (scout, stored) = kernel
            .store()?
            .write(Durability::Synced, move |tx| {
                // An unfinished scout goes on with the files it began with.
                // A call that names other files is another scout.
                if !fresh
                    && let Some(scout) = newest_scout(tx, &workspace, &person, &named, &project, false)?
                {
                    match stored_files(tx, scout.id)? {
                        None => return Ok((scout, None)),
                        Some(Some(files)) if asking.is_empty() || files == asking => {
                            return Ok((scout, Some(files)));
                        }
                        Some(_) => {}
                    }
                }
                let id = RunId::from_parts(tx.now(), tx.random()?)?;
                let now = tx.now().as_unix_millis();
                tx.tables(Owner::Actions).execute(
                    "INSERT INTO actions_scouts (scout_id, workspace_id, principal_id, target, started_at, project)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    &[
                        SqlValue::from(id.to_string()),
                        SqlValue::from(workspace.to_string()),
                        SqlValue::from(person.to_string()),
                        SqlValue::from(named.as_str()),
                        SqlValue::from(now),
                        SqlValue::from(project.as_str()),
                    ],
                )?;
                Ok((
                    Scout {
                        id,
                        finished_at: None,
                    },
                    None,
                ))
            })
            .await?;
        // The files of a scout that goes on were checked when it began; a
        // file may have changed since, so each is checked again.
        let handed = match stored {
            Some(files) if asked.is_empty() && !files.is_empty() => {
                let again = discovery_files(kernel, &call, Some(&json!(files))).await?;
                Handed {
                    files: again,
                    resumed: true,
                }
            }
            Some(files) => Handed {
                files,
                resumed: false,
            },
            None => Handed {
                files: asked,
                resumed: false,
            },
        };
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
                "files": handed.files,
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
            let identified = match identify(kernel, &call, &target, None, true, &handed).await {
                Ok(identified) => identified,
                // Drafts that do not compile declare no surface the engine
                // can observe: the scout reports that and goes on, so that
                // a target just added gets a report and not an error.
                Err(failure) if failure.code() == &a2m_files::LINT_FAILED => Identified {
                    compiled_target_hash: Digest::of(b"no plan"),
                    surfaces: Vec::new(),
                    runs: BTreeMap::new(),
                    complete: true,
                    not_compiled: Some(failure.message().to_owned()),
                    sign_in_findings: BTreeMap::new(),
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
            // A surface that cannot be observed until its person signs in
            // stops the scout by name, before anything is drafted from a
            // look that missed it. The stage is not stored: the next call
            // of the same scout identifies again, under the session.
            if let Some(waiting) = identified
                .surfaces
                .iter()
                .find(|surface| surface["state"] == "needs_login")
            {
                return Ok(needs_sign_in(&target, id, waiting, &done));
            }
            let declared = declared_sign_ins(kernel, &call, &target).await?;
            let mut result = json!({
                "compiled_target_hash": identified.compiled_target_hash.to_string(),
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
        go_on(kernel, &call, &target, id, done, folder).await
    }
);

/// The stages of a scout after the identify stage, each stored before the
/// next begins: what was found, compared, drafted, checked, tried, and the
/// report. A scout of the engine's discovery and a scout of a capture go
/// through them alike (decision 0400).
#[allow(
    clippy::too_many_lines,
    reason = "the stages in their order; split, each part would take the whole state"
)]
async fn go_on(
    kernel: &Kernel,
    call: &Call,
    target: &TargetName,
    id: RunId,
    mut done: BTreeMap<u8, Json>,
    folder: Option<PathBuf>,
) -> Result<Output, Failure> {
    let workspace = call.caller.principal.workspace;
    let runs: BTreeMap<String, String> =
        serde_json::from_value(done[&1]["runs"].clone()).unwrap_or_default();
    let (findings, by_discoverer) = findings_of(kernel, workspace, target, &runs).await?;
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
        let (compared, mut asked) = described_against_observed(kernel, &findings);
        let mut result = match &folder {
            Some(_) => {
                let (result, drafted) = compare(kernel, call, target, id, findings.clone()).await?;
                asked.extend(drafted);
                result
            }
            None => no_folder.clone(),
        };
        if let Json::Object(map) = &mut result {
            map.insert(DESCRIBED_AGAINST_OBSERVED.to_owned(), compared);
        }
        let named = target.clone();
        kernel
            .store()?
            .write(Durability::Synced, move |tx| {
                open_questions(tx, workspace, &named, &asked)
            })
            .await?;
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
                    target,
                    CompileMode::Draft,
                    DRAFT_VERSION,
                )
                .await?
                {
                    Ok(compiled) => json!({
                        "compiles": true,
                        "problems": marked_problems(&compiled.warnings),
                        "tools": compiled.compiled_target.tools.iter().map(|tool| tool.name.to_string()).collect::<Vec<_>>(),
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
            let tools = drafted_tools(kernel, folder.clone(), written, held_back.clone()).await?;
            let Tried {
                mut result,
                runs,
                complete,
            } = try_reads(kernel, call, id, target, folder.clone(), &tools, held_back).await?;
            if !complete {
                return Ok(stopped(
                    target,
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
        let report = super::scout_report::assemble(target, id, &done, &open);
        kernel
            .store_stage(id, 7, input_hash(&done[&6]), &report)
            .await?;
        done.insert(7, report);
    }
    let report = done.remove(&7).unwrap_or(Json::Null);
    Ok(finished(&report))
}

/// A finished scout's report as `scout.run` answers it (copy test 9,
/// finding 56). A scout whose discovery of every surface it observed
/// failed saw nothing, and answers with that failure beside the report:
/// the class and code the first such surface stopped with, and the fix its
/// failure declares where the report keeps one. A scout where some surface
/// failed and another did not answers `ok`, with a warning naming each
/// that failed.
fn finished(report: &Json) -> Output {
    let output = super::scout_report::shown(report);
    let observed = super::scout_report::observed(report);
    let failed: Vec<&(String, Option<Json>)> = observed
        .iter()
        .filter(|(_, failure)| failure.is_some())
        .collect();
    if failed.is_empty() {
        return output;
    }
    if failed.len() < observed.len() {
        return failed.iter().fold(output, |output, (name, failure)| {
            output.warn(super::scout_report::failed_line(name, failure.as_ref()))
        });
    }
    let Some((name, Some(failure))) = failed.first().copied() else {
        return output;
    };
    let class = failure["class"]
        .as_str()
        .and_then(|class| class.parse().ok());
    let code = failure["code"]
        .as_str()
        .and_then(|code| a2m_core::FailureCode::new(code).ok());
    let (Some(class), Some(code)) = (class, code) else {
        return output.warn(super::scout_report::failed_line(name, Some(failure)));
    };
    let fix = failure["fix"].as_str().map_or_else(
        || "Read `not_looked_at` in the report, which names what stopped each surface, and run the scout again once it is put right.".to_owned(),
        str::to_owned,
    );
    output.failed(Failure::new(
        class,
        code.clone(),
        format!(
            "The discovery of every surface the scout observed failed, so nothing was found to draft from: the surface `{}` stopped with `{code}`.",
            codes::shown(name)
        ),
        fix,
    ))
}

/// What a capture came to, as the scout of it begins (decision 0400): the
/// capture's discoverers ran, a person drove them, and their findings are
/// stored under the capture's identifier, as a discovery's are.
pub(crate) struct Captured {
    /// The scout the capture's report is kept under, claimed when the
    /// capture ended.
    pub(crate) scout: RunId,
    /// The capture's identifier, under which its discoverers reported.
    pub(crate) capture_id: String,
    pub(crate) target: TargetName,
    pub(crate) surface: SurfaceName,
    /// The compiled target the capture was asked over, and its hash.
    pub(crate) compiled_target: Arc<CompiledTarget>,
    pub(crate) compiled_target_hash: Digest,
    /// How it ended, by its name (decision 0433, point 5).
    pub(crate) ended_by: &'static str,
    /// When its person began it, and when it ended, by the engine's clock.
    pub(crate) began_at: Option<i64>,
    pub(crate) ended_at: i64,
    /// What each discoverer reported: its provider, its name, and its
    /// report as a discovery's is kept.
    pub(crate) reported: Vec<(String, String, Json)>,
}

/// A scout of a capture: the same stages as `scout.run`'s, its identify
/// and discover stages the capture's own (decision 0400, point 1). The
/// first two stages say what the capture was; from the third on it is a
/// scout's, stored stage by stage, so that a call again, of
/// `capture.stop` or of `scout.run`, goes on from the last stage stored.
/// Nothing reaches the system but the drafted reads' trials, in the
/// sandbox.
pub(crate) async fn scout_capture(
    kernel: &Kernel,
    call: &Call,
    captured: Captured,
) -> Result<Output, Failure> {
    let principal = &call.caller.principal;
    let (workspace, person) = (principal.workspace, principal.principal.id);
    let id = captured.scout;
    let (named, project) = (captured.target.clone(), project_key(call));
    let stages = kernel
        .store()?
        .write(Durability::Synced, move |tx| {
            let now = tx.now().as_unix_millis();
            tx.tables(Owner::Actions).execute(
                "INSERT INTO actions_scouts (scout_id, workspace_id, principal_id, target, started_at, project)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (scout_id) DO NOTHING",
                &[
                    SqlValue::from(id.to_string()),
                    SqlValue::from(workspace.to_string()),
                    SqlValue::from(person.to_string()),
                    SqlValue::from(named.as_str()),
                    SqlValue::from(now),
                    SqlValue::from(project.as_str()),
                ],
            )?;
            stored_stages(tx, id)
        })
        .await?;
    let mut done = stages;
    let target = captured.target.clone();
    if !done.contains_key(&0) || !done.contains_key(&1) {
        // What the capture's discoverers found, as they stored it.
        let run = captured.capture_id.clone();
        let named = target.clone();
        let (found, ended) = kernel
            .store()?
            .read(move |tx| {
                let mut found = Vec::new();
                let mut ended = Vec::new();
                for reported in discovery_results(tx, &run)? {
                    for id in &reported.findings {
                        if let Some(finding) = tx.findings().get(&workspace, &named, id)? {
                            found.push(finding);
                        }
                    }
                    ended.push(json!({
                        "provider": reported.provider,
                        "discoverer": reported.discoverer,
                        "findings": reported.findings.len(),
                        "report": reported.report,
                    }));
                }
                Ok((found, ended))
            })
            .await?;
        let mut by_kind = BTreeMap::<String, u64>::new();
        let mut by_level = BTreeMap::<String, BTreeMap<String, u64>>::new();
        for finding in &found {
            *by_kind.entry(finding.kind().to_string()).or_default() += 1;
            *by_level
                .entry(finding.kind().to_string())
                .or_default()
                .entry(finding.level().as_str().to_owned())
                .or_default() += 1;
        }
        let bounds_hit: BTreeSet<String> = captured
            .reported
            .iter()
            .flat_map(|(_, _, report)| {
                report["bounds_hit"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Json::as_str)
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .collect();
        if let std::collections::btree_map::Entry::Vacant(slot) = done.entry(0) {
            let result = json!({
                "project": project_key(call),
                "files": {},
                "from": "capture",
                "capture": {
                    "capture_id": captured.capture_id,
                    "ended_by": captured.ended_by,
                    "began_at": captured.began_at,
                    "ended_at": captured.ended_at,
                    "ran_ms": captured.began_at.map(|began| captured.ended_at.saturating_sub(began).max(0)),
                    "bounds_hit": bounds_hit,
                    "findings_by_kind": by_kind,
                },
                "consent": {
                    "recorded": false,
                    "asked_by": if call.caller.is_agent { "agent" } else { "person" },
                    "why": "A capture records what its person did, once they began it on the engine's page. The scout of it reaches the system only through trials of drafted reads, in the engine's sandbox, where nothing can be written.",
                },
                "decision": call.decision.to_string(),
                "bounds": "Each discoverer of the capture was held to the bounds its manifest declares, and each trial is held to the step limits of its tool.",
                "budget_in_force": false,
                "why_not": "A capture is held to its discoverers' own bounds; no part of the engine keeps a budget for a whole scout yet.",
            });
            kernel
                .store_stage(id, 0, input_hash(&json!(target.as_str())), &result)
                .await?;
            slot.insert(result);
        }
        if !done.contains_key(&1) {
            let look = Look::of(kernel, &captured.compiled_target)
                .into_iter()
                .find(|look| look.name == captured.surface);
            let failure = captured
                .reported
                .iter()
                .all(|(_, _, report)| report.get("failure").is_some())
                .then(|| captured.reported.first())
                .flatten()
                .map(|(_, _, report)| {
                    json!({"class": report["failure"]["class"], "code": report["failure"]["code"]})
                });
            let mut entry = json!({
                "surface": captured.surface.as_str(),
                "kind": look.as_ref().map(|look| look.kind.clone()),
                "access_kind": look.as_ref().and_then(|look| look.access_kind),
                "provider": look.as_ref().and_then(|look| look.provider.clone()),
                "keeps_a_session": look.as_ref().is_some_and(|look| look.keeps_a_session),
                "observed_by": "person",
                "discoverers": captured
                    .reported
                    .iter()
                    .map(|(provider, discoverer, _)| json!({"provider": provider, "discoverer": discoverer}))
                    .collect::<Vec<_>>(),
                "session": match look.as_ref().and_then(|look| look.signs_in_with.as_ref()) {
                    Some(provider) => json!({"needed": true, "provider": provider}),
                    None => json!({"needed": false}),
                },
                "findings_by_kind": by_level,
                "ended": ended,
                "state": if failure.is_some() { "failed" } else { "identified" },
            });
            if let Some(failure) = failure {
                entry["failure"] = failure;
            }
            let result = json!({
                "compiled_target_hash": captured.compiled_target_hash.to_string(),
                "surfaces": [entry],
                "sign_in": [],
                "runs": {captured.surface.as_str(): captured.capture_id},
            });
            kernel
                .store_stage(id, 1, input_hash(&done[&0]), &result)
                .await?;
            done.insert(1, result);
        }
    }
    let folder = folder_of(call, &target).await?;
    go_on(kernel, call, &target, id, done, folder).await
}

/// The name `spec.import` hands its file under: the name a discoverer
/// declares for a description a system publishes about itself (decision
/// 0263). A target none of whose discoverers reads it is refused by
/// `scout.run`, with the names they do read.
const IMPORTED_AS: &str = "description";

handler!(
    /// `spec.import`: exactly `scout.run` on the target, fresh, handed the
    /// one file the call names under [`IMPORTED_AS`], with no logic of its
    /// own (decision 0263). The scout drafts from the description by the
    /// packs' rules and tries the drafted reads; an unfinished one goes on
    /// with `scout.run`, which hands it the same file.
    spec_import,
    |kernel, call| {
        let mut call = call;
        let file = call.required("file")?.to_owned();
        call.input.remove("file");
        call.input
            .insert("files".to_owned(), json!({ IMPORTED_AS: file }));
        call.input.insert("fresh".to_owned(), json!(true));
        scout_run(kernel, call).await
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
        // A scout named, as `capture.stop` gives its report's reference
        // (decision 0400, point 2): it is read only when it is the
        // caller's person's, of this target, in this project.
        let asked: Option<RunId> = match call.text("scout") {
            Some(text) => Some(text.parse().map_err(|_| no_such_scout(&target))?),
            None => None,
        };
        let found = kernel
            .store()?
            .read(move |tx| {
                let scout = match asked {
                    Some(id) => named_scout(tx, &workspace, &person, &named, &project, id)?
                        .filter(|scout| scout.finished_at.is_some()),
                    None => newest_scout(tx, &workspace, &person, &named, &project, true)?,
                };
                let Some(scout) = scout else {
                    return Ok(None);
                };
                let unfinished = match asked {
                    Some(_) => None,
                    None => newest_scout(tx, &workspace, &person, &named, &project, false)?,
                };
                Ok(Some((stored_stages(tx, scout.id)?, unfinished)))
            })
            .await?;
        let Some((mut stages, unfinished)) = found else {
            return Err(no_such_scout(&target));
        };
        let report = stages.remove(&7).unwrap_or(Json::Null);
        if let Some(path) = call.text("part") {
            let limit = usize::try_from(call.int("limit").unwrap_or(50)).unwrap_or(50);
            let offset = usize::try_from(call.int("offset").unwrap_or(0)).unwrap_or(0);
            return Ok(super::scout_report::page(&report, path, limit, offset));
        }
        let mut output = super::scout_report::shown(&report);
        // A surface whose discovery failed is named, as `scout.run` names
        // it; reading the report succeeds all the same (finding 56).
        for (name, failure) in super::scout_report::observed(&report) {
            if failure.is_some() {
                output = output.warn(super::scout_report::failed_line(&name, failure.as_ref()));
            }
        }
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
    /// Copy test 9, finding 56: a scout that observed surfaces and whose
    /// discovery of every one failed answers with the first one's failure,
    /// beside the report; one where another surface found something
    /// answers `ok`, with a warning that names the surface that failed. A
    /// surface no discoverer suits was not observed, and counts for
    /// neither.
    #[test]
    fn a_scout_answers_ok_only_when_a_surface_it_observed_did_not_fail() {
        use super::{ExitClass, finished, json};
        let failed = |name: &str| {
            json!({"surface": name, "state": "failed",
                "failure": {"class": "unknown_variant", "code": "browser_not_owned"},
                "ended": [{"report": {"failure": {"class": "unknown_variant", "code": "browser_not_owned", "fix": "Check the browser."}}}]})
        };
        // A run that ended, every discoverer of which failed before it
        // found anything.
        let none_found = |name: &str| {
            json!({"surface": name, "state": "identified", "findings_by_kind": {},
                "ended": [{"report": {"failure": {"class": "transient", "code": "timeout", "fix": "Try again."}}}]})
        };
        let found = |name: &str| {
            json!({"surface": name, "state": "identified",
                "findings_by_kind": {"screen": {"discovered": 2}},
                "ended": [{"report": {"coverage": "partial", "bounds_hit": [], "dropped": 0}}]})
        };
        let unsuited = json!({"surface": "files", "state": "no_discoverer"});
        let report = |surfaces: Vec<serde_json::Value>| json!({"target": "example", "report_hash": "h", "content": {"surfaces": surfaces}});

        let alone = finished(&report(vec![failed("app"), unsuited.clone()]));
        assert_eq!(alone.exit, ExitClass::Failed);
        let failure = alone.failure.unwrap();
        assert_eq!(failure.code().as_str(), "browser_not_owned");
        assert_eq!(failure.class().as_str(), "unknown_variant");
        assert_eq!(failure.next_step(), "Check the browser.");
        assert!(failure.message().contains("`app`"), "{failure}");
        assert_eq!(alone.result["report_hash"], "h");

        let both = finished(&report(vec![failed("app"), none_found("api")]));
        assert_eq!(both.failure.unwrap().code().as_str(), "browser_not_owned");

        let partly = finished(&report(vec![failed("app"), found("api")]));
        assert_eq!(partly.exit, ExitClass::Succeeded);
        assert!(partly.failure.is_none());
        assert_eq!(partly.warnings.len(), 1, "{:?}", partly.warnings);
        assert!(
            partly.warnings[0].contains("`app`")
                && partly.warnings[0].contains("browser_not_owned"),
            "{:?}",
            partly.warnings
        );

        let unobserved = finished(&report(vec![unsuited]));
        assert_eq!(unobserved.exit, ExitClass::Succeeded);
        assert!(unobserved.warnings.is_empty());
    }

    #[test]
    fn a_read_that_waits_for_a_sign_in_is_waiting_and_the_counts_add_up() {
        use super::{TRIES, json, trial_state, tried_counts};
        let rest = |status: &str| json!({"status": status, "result": {"n": 1}});
        assert_eq!(trial_state(&[rest("needs_login")], false), "waiting");
        assert_eq!(trial_state(&[rest("needs_login")], true), "not_ended");
        assert_eq!(trial_state(&[rest("failed")], false), "failed");
        let proven: Vec<_> = (0..TRIES).map(|_| rest("completed")).collect();
        assert_eq!(trial_state(&proven, false), "proven");
        let tried = [
            json!({"state": "proven"}),
            json!({"state": "waiting"}),
            json!({"state": "waiting"}),
            json!({"state": "failed"}),
            json!({"state": "differs"}),
            json!({"state": "not_tried"}),
        ];
        let counts = tried_counts(&tried, &json!(["held/a.yaml"]));
        assert_eq!(
            counts,
            json!({"drafted": 7, "proven": 1, "waiting": 2, "failed": 2, "not_tried": 1, "held": 1})
        );
    }

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
