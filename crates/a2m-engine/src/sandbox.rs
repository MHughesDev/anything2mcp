//! The sandbox, as a discoverer sees it: a port through which a write is
//! impossible.
//!
//! The kernel's half of "cannot write" is held three times. The port's one
//! method takes no authority, so there is nowhere to pass an intent token.
//! The driver was prepared in the `Discovery` posture, in which the handle
//! refuses every step that commits. And the port itself refuses such a step
//! before the handle sees it. A discovery job asks `Control` for no lock
//! and no intent, so no token exists to misuse.

use std::sync::Arc;
use std::time::Duration;

use a2m_core::{
    EffectClass, Failure, FailureClass, Finding, Hazard, OperationName, RunState, Step,
    SurfaceIndex,
};
use a2m_provider::{
    BoxFuture, CheckedPaths, Deadline, DiscoveryScope, FindingSink, Posture, ReadOnlyPort,
    SandboxPort, StepFailure, StepInput, StepOutcome, SurfaceDescription,
};
use a2m_telemetry::ChoiceReason;

use crate::codes;
use crate::control::{Control, EngineEvent, GuardQuestion};
use crate::engine::Engine;
use crate::job::{JobOutcome, JobSpec, ToolRef};
use crate::run::{Candidate, Run};

/// The name discovery is asked about at guard: the built-in tool.
const DISCOVER_TOOL: &str = "a2m_discover";

/// The port a discoverer is handed.
struct Sandbox {
    inner: Option<ReadOnlyPort>,
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
                    FailureClass::Policy,
                    codes::SANDBOX_READ_ONLY,
                    "A discoverer asked for a step that commits, and discovery writes nothing.",
                    "Report this to the provider's author; a discoverer only reads.",
                )));
            }
            let Some(port) = &self.inner else {
                return Err(StepFailure::not_sent(codes::fail(
                    FailureClass::Busy,
                    codes::NO_BINDING,
                    "No driver is ready for the surface, so nothing on it can be observed.",
                    "Enable a provider that drives a mode of the surface.",
                )));
            };
            port.act(step, input).await
        })
    }
}

struct Collected(Vec<Finding>);

impl FindingSink for Collected {
    fn found(&mut self, finding: Finding) -> Result<(), Failure> {
        self.0.push(finding);
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
        let loaded = self.plan(&job.plan, control).await?;
        let position = Self::environment(&loaded, job)?;
        let plan = &loaded.plan;
        let surface_position = plan
            .surfaces
            .iter()
            .position(|surface| &surface.name == name)
            .ok_or_else(|| {
                codes::job_invalid(format!("The plan holds no surface named `{name}`."))
            })?;
        let surface = &plan.surfaces[surface_position];
        let registry = &self.deps.registry;
        let entry = registry.surface_kind(&surface.kind).ok_or_else(|| {
            codes::job_invalid(format!(
                "No enabled provider serves the surface kind `{}`.",
                surface.kind
            ))
        })?;
        let archetype = entry.archetype;
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

        // The driver behind the port: that of the first binding in the plan
        // that acts on this surface and whose driver the registry has. It
        // is prepared in the `Discovery` posture.
        let mut port = Sandbox { inner: None };
        let binding = plan
            .operations
            .iter()
            .flat_map(|operation| &operation.bindings)
            .find(|binding| {
                binding.surface.get() == surface_position && registry.driver(&binding.mode).is_ok()
            });
        let surface_ref = run.surface_at(SurfaceIndex::new(
            u32::try_from(surface_position).unwrap_or(u32::MAX),
        ))?;
        if let Some(binding) = binding {
            let handle = registry.driver(&binding.mode)?;
            let candidate = Candidate {
                binding: 0,
                reason: ChoiceReason::FirstInOrder,
                handle: handle.clone(),
            };
            let held = run.hold(&tool, binding, &candidate, false).await?;
            port.inner = Some(ReadOnlyPort::new(handle, run.take(held)));
        }
        let description = SurfaceDescription {
            kind: surface.kind.clone(),
            archetype,
            address: surface_ref.surface.address.clone(),
            facts: job.input.clone(),
        };
        let paths = CheckedPaths::none();
        let mut outcome = Ok(());
        for discoverer in registry.discoverers_for(&description) {
            let declared = discoverer.declared();
            let scope = DiscoveryScope {
                surface: &surface_ref,
                bounds: declared.bounds,
                deadline: Deadline::after(Duration::from_secs(u64::from(declared.bounds.seconds))),
                paths: &paths,
                redact: self.deps.redact.as_ref(),
                focus: &job.input,
            };
            let mut sink = Collected(Vec::new());
            let report = discoverer.discover(&port, &scope, &mut sink).await;
            let sent = control
                .send(EngineEvent::Discovered {
                    surface: name.clone(),
                    provider: discoverer.provider().to_string(),
                    discoverer: declared.name.to_string(),
                    findings: sink.0,
                    report,
                })
                .await;
            if let Err(failure) = sent {
                outcome = Err(failure);
                break;
            }
        }
        if let Some(inner) = port.inner.take() {
            let (handle, prepared) = inner.into_parts();
            let _ = handle.release(prepared, outcome.is_err()).await;
        }
        outcome
    }
}
