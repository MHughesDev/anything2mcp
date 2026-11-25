//! The sandbox, as a discoverer sees it: a port through which a write is
//! impossible.
//!
//! The kernel's half of "cannot write" is held three times. The port's one
//! method takes no authority, so there is nowhere to pass an intent token.
//! The driver was prepared in the `Discovery` posture, in which the handle
//! refuses every step that commits. And the port itself refuses such a step
//! before the handle sees it. A discovery job asks `Control` for no lock
//! and no intent, so no token exists to misuse.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;

use a2m_core::{
    EffectClass, Failure, Finding, Hazard, OperationName, Redact, RunState, Step, SurfaceIndex,
};
use a2m_logs::ChoiceReason;
use a2m_provider::{
    Access, BoxFuture, CheckedPaths, Deadline, DiscoveryScope, EntryKind, FindingSink, Posture,
    ReadOnlyPort, RootName, Roots, SandboxPort, StepFailure, StepInput, StepOutcome,
    SurfaceDescription,
};

use crate::codes;
use crate::control::{Control, EngineEvent, GuardQuestion};
use crate::engine::Engine;
use crate::guard::{Timed, caught};
use crate::job::{JobOutcome, JobSpec, ToolRef};
use crate::run::{Candidate, Run};

/// The name discovery is asked about at guard: the built-in tool.
const DISCOVER_TOOL: &str = "a2m_discover";

/// The port a discoverer is handed: the surface's own driver, read only,
/// or why there is none.
struct Sandbox {
    inner: Result<ReadOnlyPort, Failure>,
}

impl SandboxPort for Sandbox {
    fn act<'a>(
        &'a self,
        step: &'a Step,
        input: &'a StepInput<'a>,
    ) -> BoxFuture<'a, Result<StepOutcome, StepFailure>> {
        Box::pin(async move {
            if step.commits {
                return Err(StepFailure::not_sent(codes::fail(
                    &codes::row::SANDBOX_READ_ONLY,
                    "A discoverer asked for a step that commits, and discovery writes nothing.",
                    "Report this to the provider's author; a discoverer only reads.",
                )));
            }
            match &self.inner {
                Ok(port) => port.act(step, input).await,
                Err(why) => Err(StepFailure::not_sent(why.clone())),
            }
        })
    }
}

/// The most findings one discoverer may hand over in one job, whatever
/// their size.
const MOST_FINDINGS: usize = 10_000;

/// Where a discoverer's findings go. It keeps them up to the size its
/// manifest declares and up to a fixed count; what comes after is counted
/// and not kept, so a discoverer that floods cannot grow the engine's
/// memory without bound. A finding that still holds a registered secret is
/// not kept either: nothing of it leaves the engine.
struct Collected<'r> {
    redact: &'r dyn Redact,
    kept: Vec<Finding>,
    /// The bytes the kept findings take, as they are stored.
    bytes: u64,
    /// The most bytes that are kept.
    most_bytes: u64,
    /// How many findings came after the bound and were not kept.
    over: u64,
    /// How many findings held a registered secret and were not kept.
    withheld: u64,
}

impl<'r> Collected<'r> {
    fn new(redact: &'r dyn Redact, most_bytes: u64) -> Self {
        Self {
            redact,
            kept: Vec::new(),
            bytes: 0,
            most_bytes,
            over: 0,
            withheld: 0,
        }
    }
}

impl FindingSink for Collected<'_> {
    fn found(&mut self, finding: Finding) -> Result<(), Failure> {
        // A finding that cannot be written out cannot be measured or
        // checked: it is counted as not kept.
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

impl Engine {
    /// Carries out a discovery job: every registered discoverer that suits
    /// the job's surface is handed the sandbox as its port, and what each
    /// finds is sent through `Control`.
    pub async fn discover(&self, job: &JobSpec, control: &dyn Control) -> JobOutcome {
        match self.discovery(job, control).await {
            Ok(()) => JobOutcome::Finished {
                state: RunState::Completed,
                failure: None,
            },
            Err(failure) => JobOutcome::Left(failure),
        }
    }

    async fn discovery(&self, job: &JobSpec, control: &dyn Control) -> Result<(), Failure> {
        let ToolRef::Surface(name) = &job.tool else {
            return Err(codes::job_invalid(
                "A discovery job names a surface, and this one names a tool.",
            ));
        };
        let timed = Timed::new(control, self.deps.settings.control_timeout);
        let control: &dyn Control = &timed;
        let loaded = self.compiled_target(&job.compiled_target, control).await?;
        let position = Self::environment(&loaded, job)?;
        let compiled_target = &loaded.compiled_target;
        let surface_position = compiled_target
            .surfaces
            .iter()
            .position(|surface| &surface.name == name)
            .ok_or_else(|| {
                codes::job_invalid(format!(
                    "The compiled target holds no surface named `{name}`."
                ))
            })?;
        let surface = &compiled_target.surfaces[surface_position];
        let registry = &self.deps.registry;
        let entry = registry.surface_kind(&surface.kind).ok_or_else(|| {
            codes::job_invalid(format!(
                "No enabled provider serves the surface kind `{}`.",
                surface.kind
            ))
        })?;
        let access_kind = entry.access_kind;
        let handed = control.discovery_files();
        let mut run = Run::new(
            self,
            job,
            control,
            Arc::clone(&loaded),
            position,
            Posture::Discovery,
            None,
        );
        run.moved(RunState::Validating).await?;
        let surface_ref = run.surface_at(SurfaceIndex::new(
            u32::try_from(surface_position).unwrap_or(u32::MAX),
        ))?;
        let description = SurfaceDescription {
            kind: surface.kind.clone(),
            access_kind,
            address: surface_ref.surface.address.clone(),
            facts: job.input.clone(),
        };
        // The project folder of the request that started the discovery, as
        // every step of the run is handed it (decision 0196), and the files
        // of its `inputs` folder the call named, each checked again here,
        // before policy is asked or a driver is held: one that fails its
        // check ends the job, and no discoverer runs (decision 0263).
        let paths = input_files(run.paths(), &handed, &registry.files_taken(&description))?;
        let tool = OperationName::new(DISCOVER_TOOL)?;
        control
            .decide(&GuardQuestion {
                tool: tool.clone(),
                environment: run.environment.clone(),
                effect: EffectClass::Read,
                hazard: Hazard::None,
                writes: false,
                records: None,
            })
            .await?;
        run.moved(RunState::Running).await?;

        // The driver behind the port is the surface's own: discovery is
        // about a surface, not a binding, so a target that declares only
        // its surface is observed all the same. It is prepared in the
        // `Discovery` posture.
        let mut port = Sandbox {
            inner: Err(codes::fail(
                &codes::row::NO_BINDING,
                "No driver is ready for the surface, so nothing on it can be observed.",
                "Enable a provider that drives a mode of the surface.",
            )),
        };
        match surface_mode(registry, &surface.kind, access_kind, &entry.provider) {
            Ok((mode, handle)) => {
                let candidate = Candidate {
                    binding: 0,
                    reason: ChoiceReason::FirstInOrder,
                    handle: handle.clone(),
                };
                let at = SurfaceIndex::new(u32::try_from(surface_position).unwrap_or(u32::MAX));
                let held = run
                    .hold_on(&tool, &mode, at, &candidate, false, false)
                    .await?;
                port.inner = Ok(ReadOnlyPort::new(handle, run.take(held)));
            }
            Err(why) => port.inner = Err(why),
        }
        let mut outcome = Ok(());
        for discoverer in registry.discoverers_for(&description) {
            let declared = discoverer.declared();
            // The time a discoverer may take is the time its manifest
            // declares, and never more than the job's lease has left.
            let declared_time = Duration::from_secs(u64::from(declared.bounds.seconds));
            let time = job
                .lease
                .remaining()
                .map_or(declared_time, |left| left.min(declared_time));
            let scope = DiscoveryScope {
                surface: &surface_ref,
                bounds: declared.bounds,
                deadline: Deadline::after(time),
                paths: &paths,
                redact: self.deps.redact.as_ref(),
                focus: &job.input,
            };
            let mut sink = Collected::new(self.deps.redact.as_ref(), declared.bounds.bytes);
            // A discoverer is a provider's code: it is held to the time its
            // manifest declares, and a panic in it ends it and not the job.
            // Every step it asks for only reads, so its future may be
            // dropped.
            let looking = caught(discoverer.discover(&port, &scope, &mut sink));
            let report = match tokio::time::timeout_at(scope.deadline.at(), looking).await {
                Ok(Some(report)) => report,
                Ok(None) => Err(codes::fail(
                    &codes::row::DRIVER_PANICKED,
                    format!("The discoverer `{}` panicked.", declared.name),
                    "Report this to the provider's author; the findings made before stand.",
                )),
                Err(_) => Err(codes::fail(
                    &codes::row::RUN_TIME_EXCEEDED,
                    format!(
                        "The discoverer `{}` did not end within the time its manifest declares.",
                        declared.name
                    ),
                    "Narrow what is observed, or raise the discoverer's bound.",
                )),
            };
            // The bound the sink held is reported with the discoverer's own.
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
            let sent = control
                .send(EngineEvent::Discovered {
                    surface: name.clone(),
                    provider: discoverer.provider().to_string(),
                    discoverer: declared.name.to_string(),
                    findings: sink.kept,
                    report,
                })
                .await;
            if let Err(failure) = sent {
                outcome = Err(failure);
                break;
            }
        }
        if let Ok(inner) = port.inner {
            let (handle, prepared) = inner.into_parts();
            let _ = caught(handle.release(prepared, outcome.is_err())).await;
        }
        outcome
    }
}

/// The folder of a project whose files a person may hand a discovery.
const INPUTS: &str = "inputs";

/// Caller's text as an error message may hold it: no control character,
/// and at most 80 characters.
fn shown(text: &str) -> String {
    const SHOWN: usize = 80;
    let clean: String = text
        .chars()
        .filter(|c| !c.is_control())
        .take(SHOWN)
        .collect();
    if text.chars().count() > SHOWN {
        format!("{clean}…")
    } else {
        clean
    }
}

/// The run's checked paths with each file a call handed a discovery added
/// under its name (decision 0263). Each path is the call's text, `inputs/`
/// and a file below it. It is joined under a root made here of the
/// project's `inputs` folder, for reading only, by
/// [`a2m_provider::CheckedPath::join`], which refuses an absolute path,
/// `..`, and a link that leads out of the folder; a part that starts with
/// a dot is refused too, and what stands at the path must be a regular
/// file, not a link. A name that no
/// discoverer of the surface reads is refused: a file nobody reads is a
/// mistake, never ignored (rule 7). Nothing is read here.
fn input_files(
    paths: &CheckedPaths,
    handed: &BTreeMap<String, String>,
    taken: &BTreeSet<String>,
) -> Result<CheckedPaths, Failure> {
    let mut checked = paths.clone();
    if handed.is_empty() {
        return Ok(checked);
    }
    let refused = |path: &str, why: &str| {
        codes::fail(
            &a2m_provider::codes::row::PATH_NOT_ALLOWED,
            format!(
                "The file `{}` handed to the discovery is refused: {why}.",
                shown(path)
            ),
            "Name a regular file inside the project's `inputs` folder, as `inputs/` and its path below it.",
        )
    };
    let project = paths.project_folder()?;
    let folder = project
        .join(INPUTS)
        .map_err(|_| refused(INPUTS, "the project's `inputs` folder cannot be used"))?;
    let root = RootName::new(INPUTS)?;
    let mut roots = Roots::none();
    roots
        .add(root.clone(), folder.as_path(), Access::Read)
        .map_err(|_| refused(INPUTS, "the project has no `inputs` folder"))?;
    let sources = roots.folder(&root, Access::Read)?;
    for (name, path) in handed {
        if !taken.contains(name) {
            let names: Vec<&str> = taken.iter().map(String::as_str).collect();
            return Err(codes::job_invalid(format!(
                "No discoverer of the surface reads a file named `{}`; those read are: {}.",
                shown(name),
                if names.is_empty() {
                    "none".to_owned()
                } else {
                    names.join(", ")
                }
            )));
        }
        let below = path
            .strip_prefix("inputs/")
            .ok_or_else(|| refused(path, "it is not in the project's `inputs` folder"))?;
        if below.split('/').any(|part| part.starts_with('.')) {
            return Err(refused(path, "a part of it is hidden"));
        }
        let file = sources.join(below)?;
        if file.entry_kind()? != EntryKind::File {
            return Err(refused(path, "it is not a regular file"));
        }
        checked.insert(name.clone(), file);
    }
    Ok(checked)
}

/// The mode a discovery of a surface is made through, and its driver: of
/// the modes the provider that serves the surface's kind declares, those of
/// the kind's access kind whose driver the registry can hand over; the one an
/// operation would try first, by the `rank` its manifest declares. Two of
/// one rank are a choice nothing states, and none is guessed (rule 7).
fn surface_mode(
    registry: &a2m_provider::Registry,
    kind: &a2m_core::SurfaceKind,
    access_kind: a2m_core::AccessKind,
    provider: &a2m_provider::ProviderName,
) -> Result<(a2m_core::Mode, a2m_provider::DriverHandle), Failure> {
    let listing = registry
        .listing()
        .into_iter()
        .find(|listing| &listing.name == provider);
    let mut ready = Vec::new();
    let mut refused = None;
    for mode in listing
        .iter()
        .flat_map(|listing| listing.manifest.modes.iter())
        .filter(|mode| mode.access_kind == access_kind)
    {
        match registry.driver(&mode.name) {
            Ok(handle) => ready.push((mode.rank, mode.name.clone(), handle)),
            Err(failure) => {
                refused.get_or_insert(failure);
            }
        }
    }
    ready.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    match ready.as_slice() {
        [] => Err(refused.unwrap_or_else(|| {
            codes::fail(
                &codes::row::NO_BINDING,
                format!(
                    "The provider `{provider}` serves the surface kind `{kind}` and drives no mode of its access kind, so nothing on the surface can be observed."
                ),
                "Enable a provider that drives a mode of the surface's kind.",
            )
        })),
        [(first, ..), (second, ..), ..] if first == second => Err(codes::fail(
            &codes::row::NO_BINDING,
            format!(
                "The provider `{provider}` drives more than one mode of the surface kind `{kind}` at the same rank, and nothing says which one observes it."
            ),
            "Give the modes different ranks in the provider's manifest.",
        )),
        [(_, mode, handle), ..] => Ok((mode.clone(), handle.clone())),
    }
}
