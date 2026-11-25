//! Captures of a person's use (decision 0392, point 2; decision 0395): an
//! agent asks for one, and only its person begins it, on the engine's
//! page, past the presence prompt; it ends when its person ends it there,
//! when the one who asked calls `capture.stop`, or at the bound its
//! discoverer's manifest declares.
//!
//! The kernel holds a capture's life, that only its person begins it, its
//! bound, and the storing of its findings. What is watched is the pack's:
//! each discoverer a person drives that suits the surface is called by
//! [`a2m_provider::DiscovererHandle::watch`] alone, with the one
//! [`a2m_provider::CaptureEnd`] the capture's stop sets. Its findings pass
//! the redactor and the bounds a discovery's pass, and are stored as a
//! discovery's are: each in the store's findings, and what each discoverer
//! reported beside them under the capture's identifier (decision 0170).
//!
//! A capture lives in the engine's memory. One that is recording when the
//! engine closes is ended then, and what it found is stored; an engine
//! that starts again has no capture under way.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use a2m_core::{
    Digest, Environment, Failure, Finding, PrincipalId, Redact, RunId, SurfaceName, TargetName,
    Timestamp, Values, WorkspaceId,
};
use a2m_ledger::Durability;
use a2m_provider::{
    CaptureStop, CheckedPaths, Deadline, DiscovererHandle, DiscoveryReport, DiscoveryScope,
    FindingSink, SurfaceDescription, SurfaceRef,
};
use serde_json::{Map, Value as Json, json};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use super::handler;
use crate::codes;
use crate::dispatch::{Caller, ExitClass, Next, Output};
use crate::kernel::Kernel;

/// The most findings one discoverer of a capture may hand over, whatever
/// their size: as for a discovery.
const MOST_FINDINGS: usize = 10_000;

/// How long `capture.stop` waits for a capture to end once it is told to:
/// the contract's grace, and a margin for storing what was found.
const STOP_WAIT: Duration = Duration::from_secs(30);

/// How a capture ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EndedBy {
    /// The one who asked for it called `capture.stop`.
    Asked,
    /// Its person ended it on its page.
    Person,
    /// The bound its discoverer's manifest declares passed.
    Bound,
    /// It was stopped before its person began it.
    NotBegun,
    /// The engine closed while it recorded.
    EngineClosed,
    /// Its discoverers returned by themselves, before anyone ended it.
    Returned,
}

impl EndedBy {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Asked => "asked",
            Self::Person => "person",
            Self::Bound => "bound",
            Self::NotBegun => "not_begun",
            Self::EngineClosed => "engine_closed",
            Self::Returned => "returned",
        }
    }
}

/// What one discoverer of a capture reported.
#[derive(Debug, Clone)]
struct Reported {
    provider: String,
    discoverer: String,
    report: Json,
}

/// What a capture came to, once it ended.
#[derive(Debug, Clone)]
struct Ended {
    by: EndedBy,
    at: Timestamp,
    findings: usize,
    reported: Vec<Reported>,
}

#[derive(Debug, Clone)]
enum Stage {
    /// Asked for; nothing records until its person begins it.
    Asked,
    /// Its person began it; its discoverers watch.
    Recording {
        stop: CaptureStop,
        began_at: Timestamp,
        /// Who asked it to end, when someone has.
        ending: Option<EndedBy>,
    },
    /// It ended.
    Ended {
        began_at: Option<Timestamp>,
        ended: Ended,
    },
}

#[derive(Debug, Clone)]
struct Capture {
    id: String,
    workspace: WorkspaceId,
    person: PrincipalId,
    target: TargetName,
    surface: SurfaceName,
    environment: Environment,
    compiled_target_hash: Digest,
    /// The compiled target it was asked over: the drafts as compiled then,
    /// or the published version.
    compiled_target: Arc<a2m_core::CompiledTarget>,
    asked_at: Timestamp,
    bound_seconds: u32,
    stage: Stage,
    /// The scout its findings go through once it has ended, claimed by the
    /// first `capture.stop` after its end (decision 0400).
    scout: Option<RunId>,
}

impl Capture {
    fn open(&self) -> bool {
        !matches!(self.stage, Stage::Ended { .. })
    }

    fn json(&self) -> Json {
        let mut map = Map::new();
        map.insert("capture_id".to_owned(), json!(self.id));
        map.insert("target".to_owned(), json!(self.target.as_str()));
        map.insert("surface".to_owned(), json!(self.surface.as_str()));
        map.insert("environment".to_owned(), json!(self.environment.as_str()));
        map.insert(
            "compiled_target_hash".to_owned(),
            json!(self.compiled_target_hash.to_string()),
        );
        map.insert("asked_at".to_owned(), json!(self.asked_at.as_unix_millis()));
        map.insert("bound_seconds".to_owned(), json!(self.bound_seconds));
        match &self.stage {
            Stage::Asked => {
                map.insert("state".to_owned(), json!("asked"));
            }
            Stage::Recording { began_at, .. } => {
                map.insert("state".to_owned(), json!("recording"));
                map.insert("began_at".to_owned(), json!(began_at.as_unix_millis()));
            }
            Stage::Ended { began_at, ended } => {
                map.insert("state".to_owned(), json!("ended"));
                if let Some(began_at) = began_at {
                    map.insert("began_at".to_owned(), json!(began_at.as_unix_millis()));
                }
                map.insert("ended_at".to_owned(), json!(ended.at.as_unix_millis()));
                map.insert("ended_by".to_owned(), json!(ended.by.as_str()));
                map.insert("findings".to_owned(), json!(ended.findings));
                map.insert(
                    "discoverers".to_owned(),
                    Json::Array(
                        ended
                            .reported
                            .iter()
                            .map(|reported| {
                                json!({
                                    "provider": reported.provider,
                                    "discoverer": reported.discoverer,
                                    "report": reported.report,
                                })
                            })
                            .collect(),
                    ),
                );
            }
        }
        Json::Object(map)
    }
}

/// The captures of this engine, by identifier, with the tasks of those
/// that record.
#[derive(Default)]
pub(crate) struct Captures {
    state: Mutex<BTreeMap<String, Capture>>,
    jobs: Mutex<Vec<JoinHandle<()>>>,
    /// Counts each capture that ends, so that a wait for one ends.
    ended: watch::Sender<u64>,
}

impl Captures {
    fn state(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Capture>> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Ends every capture that records, as the engine closes, and waits
    /// for each to store what it found.
    pub(crate) async fn end_all(&self) {
        {
            let mut state = self.state();
            for capture in state.values_mut() {
                if let Stage::Recording { stop, ending, .. } = &mut capture.stage {
                    ending.get_or_insert(EndedBy::EngineClosed);
                    stop.stop();
                }
            }
        }
        let jobs: Vec<JoinHandle<()>> = self
            .jobs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain(..)
            .collect();
        for job in jobs {
            let _ = job.await;
        }
    }
}

fn no_such_capture() -> Failure {
    codes::input(
        codes::NOT_FOUND,
        "There is no such capture of the caller's person.",
        "Ask for one with `capture.start`.",
    )
}

/// The person a caller acts for: the only person whose captures it may
/// ask for, see, or end. Never taken from the input.
fn person_of(caller: &Caller) -> Result<PrincipalId, Failure> {
    caller.principal.person().ok_or_else(|| {
        codes::policy(
            codes::PERSON_REQUIRED,
            "A capture records a person's use, and this caller acts for no person.",
            "Call as a person, or as an agent acting for one.",
        )
    })
}

/// Where a surface is reached in an environment.
fn address_in(
    compiled_target: &a2m_core::CompiledTarget,
    environment: &a2m_core::CompiledEnvironment,
    at: usize,
) -> String {
    environment
        .addresses
        .iter()
        .find(|address| address.surface.get() == at)
        .map_or_else(
            || compiled_target.surfaces[at].address.clone(),
            |address| address.address.clone(),
        )
}

/// What a capture of one surface rests on: the surface as its environment
/// reaches it, and the discoverers a person drives that suit it.
struct Planned {
    surface_ref: SurfaceRef,
    handles: Vec<DiscovererHandle>,
}

fn planned(
    kernel: &Kernel,
    compiled_target: &a2m_core::CompiledTarget,
    environment: &a2m_core::CompiledEnvironment,
    wanted: Option<&str>,
) -> Result<Planned, Failure> {
    let registry = kernel.providers();
    let mut named = false;
    for (at, surface) in compiled_target.surfaces.iter().enumerate() {
        if wanted.is_some_and(|wanted| surface.name.as_str() != wanted) {
            continue;
        }
        named = true;
        let Some(entry) = registry.surface_kind(&surface.kind) else {
            continue;
        };
        let address = address_in(compiled_target, environment, at);
        let description = SurfaceDescription {
            kind: surface.kind.clone(),
            access_kind: entry.access_kind,
            address: address.clone(),
            facts: Values::new(),
        };
        let handles = registry.capture_discoverers_for(&description);
        if handles.is_empty() {
            continue;
        }
        let mut at_environment = surface.clone();
        at_environment.address = address;
        return Ok(Planned {
            surface_ref: SurfaceRef {
                target: compiled_target.header.target.clone(),
                environment: environment.name.clone(),
                surface: at_environment,
            },
            handles,
        });
    }
    if wanted.is_some() && !named {
        return Err(codes::input(
            codes::NOT_FOUND,
            "The target declares no such surface.",
            "Name a surface the target's files declare, or leave `surface` out.",
        ));
    }
    Err(codes::input(
        codes::CAPTURE_NOT_OFFERED,
        format!(
            "No enabled pack watches a person's use of {} of the target `{}`.",
            wanted.map_or_else(
                || "any surface".to_owned(),
                |wanted| format!("the surface `{}`", codes::shown(wanted))
            ),
            compiled_target.header.target
        ),
        "Scout the target with `scout.run` instead, or enable a pack whose discoverer a person drives.",
    ))
}

/// Where a capture's findings go as they are made: as a discovery's sink,
/// it keeps them up to the size the manifest declares and up to a fixed
/// count, and never one that holds a registered secret.
struct Collected {
    redact: Arc<dyn Redact>,
    kept: Vec<Finding>,
    bytes: u64,
    most_bytes: u64,
    over: u64,
    withheld: u64,
}

impl FindingSink for Collected {
    fn found(&mut self, finding: Finding) -> Result<(), Failure> {
        let Ok(text) = serde_json::to_string(&finding) else {
            self.over = self.over.saturating_add(1);
            return Ok(());
        };
        if self.redact.contains_secret(&text) {
            self.withheld = self.withheld.saturating_add(1);
            return Ok(());
        }
        let size = u64::try_from(text.len()).unwrap_or(u64::MAX);
        let fits = self.kept.len() < MOST_FINDINGS
            && self
                .bytes
                .checked_add(size)
                .is_some_and(|total| total <= self.most_bytes);
        if fits {
            self.bytes += size;
            self.kept.push(finding);
        } else {
            self.over = self.over.saturating_add(1);
        }
        Ok(())
    }
}

/// One discoverer's watch of a capture, held to its bound by its handle.
async fn watched(
    handle: DiscovererHandle,
    surface_ref: SurfaceRef,
    stop: CaptureStop,
    redact: Arc<dyn Redact>,
) -> (
    DiscovererHandle,
    Vec<Finding>,
    Result<DiscoveryReport, Failure>,
) {
    let declared = handle.declared().clone();
    let paths = CheckedPaths::none();
    let focus = Values::new();
    let scope = DiscoveryScope {
        surface: &surface_ref,
        bounds: declared.bounds,
        deadline: Deadline::after(Duration::from_secs(u64::from(declared.bounds.seconds))),
        paths: &paths,
        redact: redact.as_ref(),
        focus: &focus,
    };
    let mut sink = Collected {
        redact: Arc::clone(&redact),
        kept: Vec::new(),
        bytes: 0,
        most_bytes: declared.bounds.bytes,
        over: 0,
        withheld: 0,
    };
    let report = handle.watch(&scope, stop.end(), &mut sink).await;
    let report = report.map(|mut report| {
        if sink.over > 0 {
            if !report.bounds_hit.iter().any(|bound| bound == "bytes") {
                report.bounds_hit.push("bytes".to_owned());
            }
            report.dropped = report.dropped.saturating_add(sink.over);
        }
        report.dropped = report.dropped.saturating_add(sink.withheld);
        report
    });
    (handle, sink.kept, report)
}

impl Kernel {
    fn captures(&self) -> &Captures {
        &self.inner.captures
    }

    /// The capture `id`, when it is the caller's person's.
    fn own_capture(&self, caller: &Caller, id: &str) -> Result<Capture, Failure> {
        let person = person_of(caller)?;
        let workspace = caller.principal.workspace;
        self.captures()
            .state()
            .get(id)
            .filter(|capture| capture.person == person && capture.workspace == workspace)
            .cloned()
            .ok_or_else(no_such_capture)
    }

    /// What a person reads on the page beside a capture they are asked to
    /// begin: what is watched, how long at most, how it ends, and what is
    /// kept (decision 0392, point 1).
    pub(crate) fn capture_lines(
        &self,
        caller: &Caller,
        input: &Map<String, Json>,
    ) -> Result<Vec<String>, Failure> {
        let id = input
            .get("capture")
            .and_then(Json::as_str)
            .ok_or_else(no_such_capture)?;
        let capture = self.own_capture(caller, id)?;
        if !matches!(capture.stage, Stage::Asked) {
            return Err(codes::stale(
                codes::CAPTURE_ENDED,
                "The capture is no longer waiting to be begun.",
                "Ask for a new capture with `capture.start`.",
            ));
        }
        Ok(vec![
            format!(
                "It records your own use of the surface `{}` of the target `{}`, in the environment `{}`, in the engine's own browser, from when you carry it out here.",
                capture.surface, capture.target, capture.environment
            ),
            format!(
                "It ends when you press Stop recording on the page that follows, when the program that asked ends it, or after {} seconds at most, whichever is first.",
                capture.bound_seconds
            ),
            "The engine sends nothing while it records: every request is yours.".to_owned(),
            "What is kept: the structure of what you use, such as the paths, methods, and field names of the requests and the screens you visit, with the names a page gives its headings, links, and buttons, which can show a record's words; never a value you type, a value a field holds, or a value a request carries. Anything that holds a secret the engine knows is not kept at all.".to_owned(),
        ])
    }

    /// Ends a capture its person ends on its page. A capture that had
    /// already ended is left as it is.
    ///
    /// # Errors
    ///
    /// `input`/`not_found` when the capture is not that person's.
    pub(crate) fn end_capture_by_person(
        &self,
        id: &str,
        person: PrincipalId,
    ) -> Result<(), Failure> {
        let mut state = self.captures().state();
        let capture = state
            .get_mut(id)
            .filter(|capture| capture.person == person)
            .ok_or_else(no_such_capture)?;
        if let Stage::Recording { stop, ending, .. } = &mut capture.stage {
            ending.get_or_insert(EndedBy::Person);
            stop.stop();
        }
        Ok(())
    }

    /// Begins the capture `id` for its person, who has just proved
    /// presence on its page: each discoverer a person drives that suits
    /// the surface is asked to watch, until the capture's end.
    async fn begin_capture(&self, caller: &Caller, id: &str) -> Result<Capture, Failure> {
        let capture = self.own_capture(caller, id)?;
        match capture.stage {
            Stage::Asked => {}
            Stage::Recording { .. } => return Ok(capture),
            Stage::Ended { .. } => {
                return Err(codes::stale(
                    codes::CAPTURE_ENDED,
                    "The capture was ended before it was begun, so nothing was recorded.",
                    "Ask for a new capture with `capture.start`.",
                ));
            }
        }
        // What it rests on is looked at again: the discoverers that suit
        // the surface now.
        let compiled_target = Arc::clone(&capture.compiled_target);
        let environment = compiled_target
            .environment_named(&capture.environment)
            .ok_or_else(no_such_capture)?
            .clone();
        let planned = planned(
            self,
            &compiled_target,
            &environment,
            Some(capture.surface.as_str()),
        )?;
        let stop = self.providers().capture_stop();
        let began_at = self.inner.clock.now();
        let begun = {
            let mut state = self.captures().state();
            let Some(held) = state.get_mut(id) else {
                return Err(no_such_capture());
            };
            if !matches!(held.stage, Stage::Asked) {
                return Ok(held.clone());
            }
            held.stage = Stage::Recording {
                stop: stop.clone(),
                began_at,
                ending: None,
            };
            held.clone()
        };
        let kernel = self.clone();
        let id = id.to_owned();
        let job = tokio::spawn(async move {
            kernel.run_capture(&id, planned, stop).await;
        });
        let mut jobs = self
            .captures()
            .jobs
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        jobs.retain(|job| !job.is_finished());
        jobs.push(job);
        drop(jobs);
        Ok(begun)
    }

    /// The capture's job: every discoverer watches until the one end, and
    /// what each found is stored before the capture is called ended.
    async fn run_capture(&self, id: &str, planned: Planned, stop: CaptureStop) {
        let redact = self.redact();
        let mut watching = Vec::new();
        for handle in planned.handles {
            watching.push(tokio::spawn(watched(
                handle,
                planned.surface_ref.clone(),
                stop.clone(),
                Arc::clone(&redact),
            )));
        }
        let mut found = Vec::new();
        let mut reported = Vec::new();
        let mut bound = false;
        for task in watching {
            let Ok((handle, kept, report)) = task.await else {
                continue;
            };
            if report
                .as_ref()
                .is_ok_and(|report| report.bounds_hit.iter().any(|hit| hit == "seconds"))
            {
                bound = true;
            }
            reported.push((
                handle,
                kept.iter().map(|f| f.id().to_string()).collect::<Vec<_>>(),
                report,
            ));
            found.extend(kept);
        }
        // Every discoverer has returned, so the end is set for all of them.
        stop.stop();
        let (workspace, target, surface) = {
            let state = self.captures().state();
            match state.get(id) {
                Some(capture) => (
                    capture.workspace,
                    capture.target.clone(),
                    capture.surface.clone(),
                ),
                None => return,
            }
        };
        let shown: Vec<Reported> = reported
            .iter()
            .map(|(handle, _, report)| Reported {
                provider: handle.provider().to_string(),
                discoverer: handle.declared().name.to_string(),
                report: super::discovery_report_json(report),
            })
            .collect();
        let rows: Vec<(String, String, String, String)> = reported
            .iter()
            .zip(&shown)
            .map(|((_, ids, _), shown)| {
                (
                    shown.provider.clone(),
                    shown.discoverer.clone(),
                    json!(ids).to_string(),
                    shown.report.to_string(),
                )
            })
            .collect();
        let count = found.len();
        let stored = match self.store() {
            Ok(store) => {
                let (id, target, surface) = (id.to_owned(), target.clone(), surface.clone());
                store
                    .write(Durability::Synced, move |tx| {
                        for finding in &found {
                            tx.findings().put(&workspace, &target, finding)?;
                        }
                        let now = tx.now().as_unix_millis();
                        for (provider, discoverer, ids, report) in &rows {
                            tx.tables(a2m_ledger::Owner::Actions).execute(
                                "INSERT INTO actions_discovery_results
                                    (run_id, provider, discoverer, surface, findings, report, sent_at)
                                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                                 ON CONFLICT (run_id, provider, discoverer)
                                 DO UPDATE SET findings = excluded.findings,
                                     report = excluded.report, sent_at = excluded.sent_at",
                                &[
                                    a2m_ledger::SqlValue::from(id.as_str()),
                                    a2m_ledger::SqlValue::from(provider.as_str()),
                                    a2m_ledger::SqlValue::from(discoverer.as_str()),
                                    a2m_ledger::SqlValue::from(surface.as_str()),
                                    a2m_ledger::SqlValue::from(ids.as_str()),
                                    a2m_ledger::SqlValue::from(report.as_str()),
                                    a2m_ledger::SqlValue::from(now),
                                ],
                            )?;
                        }
                        Ok(())
                    })
                    .await
            }
            Err(failure) => Err(failure),
        };
        let mut shown = shown;
        if let Err(failure) = stored {
            // Nothing was stored: the capture says so, by the store's code,
            // and counts no finding.
            shown.push(Reported {
                provider: String::new(),
                discoverer: String::new(),
                report: super::discovery_report_json(&Err(failure)),
            });
        }
        let now = self.inner.clock.now();
        {
            let mut state = self.captures().state();
            if let Some(capture) = state.get_mut(id) {
                let (began_at, ending) = match &capture.stage {
                    Stage::Recording {
                        began_at, ending, ..
                    } => (Some(*began_at), *ending),
                    _ => (None, None),
                };
                // A discoverer that says its bound of time passed was ended
                // by the bound, whatever was asked after: a stop asked while
                // it returned, or while what it found was stored, came after
                // its end. Its own report is the evidence, not the order in
                // which the stop and the return were seen.
                let by = if bound {
                    EndedBy::Bound
                } else {
                    ending.unwrap_or(EndedBy::Returned)
                };
                capture.stage = Stage::Ended {
                    began_at,
                    ended: Ended {
                        by,
                        at: now,
                        findings: if shown.iter().any(|r| r.provider.is_empty()) {
                            0
                        } else {
                            count
                        },
                        reported: shown,
                    },
                };
            }
        }
        self.captures().ended.send_modify(|ended| *ended += 1);
    }

    /// Waits until the capture `id` has ended, at most `within`.
    async fn capture_ended(&self, id: &str, within: Duration) -> Option<Capture> {
        let mut seen = self.captures().ended.subscribe();
        let wait = async {
            loop {
                if let Some(capture) = self.captures().state().get(id)
                    && !capture.open()
                {
                    return capture.clone();
                }
                if seen.changed().await.is_err() {
                    // The sender is the kernel's own; it lives as long.
                    std::future::pending::<()>().await;
                }
            }
        };
        tokio::time::timeout(within, wait).await.ok()
    }
}

impl Kernel {
    /// What a capture that ended after its person began it hands the
    /// scout, with the scout it goes through, claimed now when no call
    /// claimed one before: `None` for a capture that recorded nothing.
    fn captured(&self, capture: &Capture) -> Result<Option<super::scout::Captured>, Failure> {
        let Stage::Ended {
            began_at: Some(began_at),
            ended,
        } = &capture.stage
        else {
            return Ok(None);
        };
        let fresh = {
            let mut bytes = [0_u8; 10];
            self.inner.random.fill(&mut bytes)?;
            RunId::from_parts(self.inner.clock.now(), bytes)?
        };
        let scout = {
            let mut state = self.captures().state();
            let held = state.get_mut(&capture.id).ok_or_else(no_such_capture)?;
            *held.scout.get_or_insert(fresh)
        };
        Ok(Some(super::scout::Captured {
            scout,
            capture_id: capture.id.clone(),
            target: capture.target.clone(),
            surface: capture.surface.clone(),
            compiled_target: Arc::clone(&capture.compiled_target),
            compiled_target_hash: capture.compiled_target_hash,
            ended_by: ended.by.as_str(),
            began_at: Some(began_at.as_unix_millis()),
            ended_at: ended.at.as_unix_millis(),
            reported: ended
                .reported
                .iter()
                .map(|reported| {
                    (
                        reported.provider.clone(),
                        reported.discoverer.clone(),
                        reported.report.clone(),
                    )
                })
                .collect(),
        }))
    }
}

/// A new capture's identifier.
fn new_id(kernel: &Kernel) -> Result<String, Failure> {
    let mut bytes = [0_u8; 16];
    kernel.inner.random.fill(&mut bytes)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

handler!(
    /// `capture.start`: an agent asks for a capture of its person's use of
    /// one surface. It is kept as asked, and its person is shown the page
    /// to begin it; nothing records until they begin it there, past the
    /// presence prompt (decision 0392, point 2). The press on that page
    /// carries the capture's identifier, and only a press can: a call that
    /// names a capture without a person's proof of presence is refused.
    capture_start,
    |kernel, call| {
        let target = TargetName::new(call.required("target")?)?;
        let person = person_of(&call.caller)?;
        if let Some(id) = call.text("capture") {
            if call.presence.is_none() {
                return Err(codes::policy(
                    codes::PERSON_REQUIRED,
                    "A capture is begun only by its person, on the engine's page, past the presence prompt.",
                    "Leave `capture` out: the call asks for a capture, and its person begins it on the page it gives.",
                ));
            }
            let id = id.to_owned();
            if kernel.own_capture(&call.caller, &id)?.target != target {
                return Err(no_such_capture());
            }
            let begun = kernel.begin_capture(&call.caller, &id).await?;
            return Ok(Output::ok(begun.json()).then(Next::new(
                "capture.stop",
                json!({"target": target.as_str(), "capture": id}),
                "to end the recording and be told what it found",
            )));
        }
        let (compiled_target_hash, compiled_target) =
            super::tools::compiled_target_to_observe(kernel, &call, &target).await?;
        let environment = compiled_target
            .default_environment()
            .ok_or_else(|| {
                codes::input(
                    codes::NOT_FOUND,
                    "The target declares no environment.",
                    "Declare an environment in the target's files.",
                )
            })?
            .clone();
        let planned = planned(kernel, &compiled_target, &environment, call.text("surface"))?;
        let surface = planned.surface_ref.surface.name.clone();
        let bound_seconds = planned
            .handles
            .iter()
            .map(|handle| handle.declared().bounds.seconds)
            .max()
            .unwrap_or(0);
        let workspace = call.caller.principal.workspace;
        // One capture of a target at a time for a person: one that waits or
        // records is given back, not asked for again.
        let open = kernel
            .captures()
            .state()
            .values()
            .find(|capture| {
                capture.workspace == workspace
                    && capture.person == person
                    && capture.target == target
                    && capture.open()
            })
            .cloned();
        let capture = match open {
            Some(capture) => capture,
            None => {
                let capture = Capture {
                    id: new_id(kernel)?,
                    workspace,
                    person,
                    target: target.clone(),
                    surface,
                    environment: environment.name.clone(),
                    compiled_target_hash,
                    compiled_target: Arc::clone(&compiled_target),
                    asked_at: kernel.inner.clock.now(),
                    bound_seconds,
                    stage: Stage::Asked,
                    scout: None,
                };
                kernel
                    .captures()
                    .state()
                    .insert(capture.id.clone(), capture.clone());
                capture
            }
        };
        let next = Next::new(
            "capture.stop",
            json!({"target": target.as_str(), "capture": capture.id}),
            "to end the recording and be told what it found; until its person begins it, nothing records",
        );
        if !matches!(capture.stage, Stage::Asked) {
            return Ok(Output::ok(capture.json()).then(next));
        }
        let def = kernel
            .actions()
            .find("capture.start")
            .cloned()
            .ok_or_else(|| {
                codes::input(
                    codes::ACTION_NOT_BUILT,
                    "`capture.start` is not in this engine's registry.",
                    "Call `action.search` to see which actions are built.",
                )
            })?;
        let mut asked = Map::new();
        asked.insert("target".to_owned(), json!(target.as_str()));
        asked.insert("surface".to_owned(), json!(capture.surface.as_str()));
        asked.insert("capture".to_owned(), json!(capture.id));
        let project = (call.project.clone(), call.project_refused.clone());
        let mut result = capture.json();
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
            Err(failure) => Output::ok(result).exit(ExitClass::PersonNeeded).warn(format!(
                "The capture waits for its person, who was not shown it on a page: {} Nothing records until they begin it there.",
                failure.message()
            )),
        };
        Ok(output.then(next))
    }
);

handler!(
    /// `capture.stop`: the one who asked ends the capture of their person,
    /// the one named or the newest of the target, and is told what it
    /// found. A capture that has ended is given back as it ended; one its
    /// person has not begun ends with nothing recorded.
    capture_stop,
    |kernel, call| {
        let target = TargetName::new(call.required("target")?)?;
        let person = person_of(&call.caller)?;
        let workspace = call.caller.principal.workspace;
        let id = match call.text("capture") {
            Some(id) => kernel.own_capture(&call.caller, id)?.id,
            None => {
                let state = kernel.captures().state();
                let mut own: Vec<&Capture> = state
                    .values()
                    .filter(|capture| {
                        capture.workspace == workspace
                            && capture.person == person
                            && capture.target == target
                    })
                    .collect();
                own.sort_by_key(|capture| (capture.open(), capture.asked_at));
                own.last()
                    .map(|capture| capture.id.clone())
                    .ok_or_else(no_such_capture)?
            }
        };
        let now = kernel.inner.clock.now();
        let waits = {
            let mut state = kernel.captures().state();
            let capture = state.get_mut(&id).ok_or_else(no_such_capture)?;
            if capture.target != target {
                return Err(no_such_capture());
            }
            match &mut capture.stage {
                Stage::Asked => {
                    capture.stage = Stage::Ended {
                        began_at: None,
                        ended: Ended {
                            by: EndedBy::NotBegun,
                            at: now,
                            findings: 0,
                            reported: Vec::new(),
                        },
                    };
                    false
                }
                Stage::Recording { stop, ending, .. } => {
                    ending.get_or_insert(EndedBy::Asked);
                    stop.stop();
                    true
                }
                Stage::Ended { .. } => false,
            }
        };
        let capture = if waits {
            kernel.capture_ended(&id, STOP_WAIT).await.ok_or_else(|| {
                codes::busy(
                    a2m_provider::codes::TIMEOUT,
                    "The capture was told to end and has not stored what it found yet.",
                    "Call `capture.stop` again in a moment.",
                )
            })?
        } else {
            kernel.own_capture(&call.caller, &id)?
        };
        let mut result = capture.json();
        let Some(captured) = kernel.captured(&capture)? else {
            // Not begun, nothing was recorded: there is nothing to scout.
            return Ok(Output::ok(result).then(Next::new(
                "capture.start",
                json!({"target": target.as_str()}),
                "to ask for a new capture; this one recorded nothing",
            )));
        };
        // A capture ends as a scout does (decision 0400): its findings go
        // through the scout's stages from the draft stage on, and the
        // answer is the report's reference.
        let scout = captured.scout;
        let made = super::scout::scout_capture(kernel, &call, captured).await?;
        let finished = made.result["report_hash"].as_str().map(str::to_owned);
        let mut reference = json!({"scout_id": scout.to_string()});
        let next = match &finished {
            Some(hash) => {
                reference["state"] = json!("finished");
                reference["report_hash"] = json!(hash);
                Next::new(
                    "scout.report",
                    json!({"target": target.as_str(), "scout": scout.to_string()}),
                    "to read the report of what the capture found, drafted, and tried",
                )
            }
            None => {
                reference["state"] = json!("working");
                reference["stage"] = made.result["stage"].clone();
                Next::new(
                    "capture.stop",
                    json!({"target": target.as_str(), "capture": id}),
                    "to go on with the report from the last stage stored",
                )
            }
        };
        if let Json::Object(map) = &mut result {
            map.insert("report".to_owned(), reference);
        }
        let mut output = Output::ok(result).then(next);
        output.warnings = made.warnings;
        if let Some(failure) = made.failure {
            output = output.failed(failure);
        }
        Ok(output)
    }
);
