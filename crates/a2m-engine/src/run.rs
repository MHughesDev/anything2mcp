//! One job in hand: the plan it runs, the drivers it has prepared, its
//! trace, and the parts of the loop every operation call goes through:
//! choose, prepare, and one step at a time through the driver's handle.

use std::sync::Arc;
use std::time::Duration;

use a2m_auth::{ExpirySign, SessionLease};
use a2m_core::{
    BindingPlan, BindingStatus, Environment, Failure, FailureClass, Mode, OperationIndex,
    OperationName, OperationPlan, REDACTED, RecordKey, RunState, SessionState, Step, SurfaceIndex,
    Value, Values,
};
use a2m_ledger::{IntentToken, RecordRef};
use a2m_provider::{
    Access, Authority, CheckedPaths, Deadline, Destinations, DriverHandle, DriverHealth, Observed,
    Posture, PrepareContext, Prepared, ProgramRef, RootName, Roots, Sent, StepFailure, StepInput,
    StepOutcome, SurfaceRef,
};
use a2m_telemetry::{
    ArtifactKind, ArtifactOutcome, ArtifactRef, CapturePoint, ChoiceReason, OfferedArtifact,
    PassStart, Phase, StepRecord, TraceBuilder, TraceOutcome, TracedValue,
};

use crate::codes;
use crate::compare::Field;
use crate::control::{Control, EngineEvent, KeptCapture, PassedOver, SessionWant};
use crate::engine::Engine;
use crate::guard::caught;
use crate::job::{Directive, JobKind, JobSpec, KernelHealth};
use crate::loaded::{Loaded, Scope};

/// Why the work on a job stopped before its end.
#[derive(Debug)]
pub(crate) enum Stop {
    /// A failure. Its class decides what becomes of the run.
    Fail(Failure),
    /// The lease is ending, or the runner was told to pause.
    LeaseEnding,
    /// The run was cancelled.
    Cancelled,
}

impl From<Failure> for Stop {
    fn from(failure: Failure) -> Self {
        Self::Fail(failure)
    }
}

/// A driver context the job holds, with the session lease it was prepared
/// under.
struct Held {
    mode: Mode,
    surface: SurfaceIndex,
    /// The addresses the context was prepared to reach: the running
    /// operation's own. A context is used again only for the same.
    destinations: Destinations,
    surface_ref: SurfaceRef,
    handle: DriverHandle,
    prepared: Prepared,
    lease: Option<SessionLease>,
    /// The posture the context was prepared in. A context is used again
    /// only for the posture it was prepared in.
    posture: Posture,
    /// Which of the job's contexts this is, counted from the first. A
    /// context prepared in place of one that failed has a new number.
    serial: u64,
    poisoned: bool,
}

/// A binding in shadow by its place, with the candidate it runs as or why
/// it may not run now.
pub(crate) type ShadowCandidate = (usize, Result<Candidate, Failure>);

/// What [`Run::set_aside`] took out of the run.
pub(crate) struct Aside {
    trace: Option<TraceBuilder>,
    pass: u32,
    next_pass: u32,
    begins: Option<PassInfo>,
}

/// A binding that qualifies, in the plan's order.
pub(crate) struct Candidate {
    pub binding: usize,
    pub reason: ChoiceReason,
    pub handle: DriverHandle,
}

/// The record a step belongs to.
#[derive(Clone, Copy)]
pub(crate) struct RecordCx<'s> {
    pub record: RecordRef,
    /// The operation the record is written or read for: what a driver's
    /// question whether another record holds a value is bound to.
    pub operation: &'s OperationName,
    /// The record's key as the ledger holds it, which may be its place in
    /// the run: for the trace, never for a step.
    pub ledger_key: &'s RecordKey,
    /// The record's key in the system: from its key inputs, or the key the
    /// system assigned. `None` while it has none; never the engine's own
    /// name for the record in the run.
    pub key: Option<&'s RecordKey>,
    pub stamp: Option<&'s str>,
    /// What the record's earlier steps returned for reconcile.
    pub recorded: &'s Values,
}

/// One step to carry out.
pub(crate) struct StepCx<'s> {
    pub held: usize,
    pub phase: Phase,
    pub position: usize,
    pub step: &'s Step,
    pub scope: &'s Scope,
    pub record: Option<RecordCx<'s>>,
    pub token: Option<&'s IntentToken>,
    pub fields: &'s [Field],
    /// For a step that commits: the time the steps after it may take. The
    /// step is sent again, after a failure that allows it, only while the
    /// lease covers its own time limit and this.
    pub then: Duration,
}

/// What is known of a pass until its first step is traced.
pub(crate) struct PassInfo {
    pub operation: OperationName,
    pub reason: ChoiceReason,
    pub queue: Duration,
    pub prepare: Duration,
}

pub(crate) struct Run<'a> {
    pub engine: &'a Engine,
    pub job: &'a JobSpec,
    pub control: &'a dyn Control,
    pub loaded: Arc<Loaded>,
    pub environment: Environment,
    pub env_position: usize,
    pub base: Scope,
    pub posture: Posture,
    /// The run's state, as far as this job has moved it.
    pub state: RunState,
    pub trace: Option<TraceBuilder>,
    pass: u32,
    next_pass: u32,
    begins: Option<PassInfo>,
    held: Vec<Held>,
    next_serial: u64,
    paths: CheckedPaths,
    /// The time on the engine's clock at which the run's own time limit
    /// passes.
    pub time_limit: Option<Duration>,
    /// For an offline test: the replay of recorded answers that every
    /// binding is handed in place of its provider's driver. No session is
    /// asked for, and no provider's driver is used (decision 0177).
    pub replay: Option<Arc<dyn a2m_provider::Driver>>,
    /// Whether a binding in shadow may run beside this job's reads: a live
    /// `Execute` job of a tool that only reads (decision 0207).
    pub shadow_allowed: bool,
    /// The operations a binding in shadow already ran beside in this job:
    /// at most once each.
    pub shadowed: Vec<OperationName>,
}

/// Whether a binding of this status may be chosen in this posture.
fn status_qualifies(status: BindingStatus, posture: Posture) -> bool {
    match posture {
        Posture::Live => status == BindingStatus::Verified,
        Posture::Sandbox { .. } | Posture::Discovery => status != BindingStatus::Retired,
    }
}

/// The failure of a binding that does not qualify for a reason that is not
/// a session, an edge block, or drift.
fn no_binding(why: String) -> Failure {
    codes::fail(
        FailureClass::Busy,
        codes::NO_BINDING,
        why,
        "Check the binding's status and its provider with `anything2mcp engine status`.",
    )
}

/// The order in which the reasons a binding did not qualify are reported:
/// the class of the best reason is the failure of the call.
fn reason_rank(class: FailureClass) -> u8 {
    match class {
        FailureClass::SessionExpired => 0,
        FailureClass::EdgeBlocked => 1,
        FailureClass::Drift => 2,
        _ => 3,
    }
}

impl<'a> Run<'a> {
    pub fn new(
        engine: &'a Engine,
        job: &'a JobSpec,
        control: &'a dyn Control,
        loaded: Arc<Loaded>,
        env_position: usize,
        posture: Posture,
        trace: Option<TraceBuilder>,
    ) -> Self {
        let plan = &loaded.plan;
        let environment = &plan.environments[env_position];
        let env_value = Value::Record(
            Values::new()
                .with("name", environment.name.as_str())
                .with("kind", environment.kind.as_str()),
        );
        // The project folder of the request that started the run, checked
        // once more as a root: a pack resolves a file of the project against
        // it, and never against the folder the engine was started in.
        let mut paths = CheckedPaths::none();
        if let Some(folder) = control.project_folder() {
            let mut roots = Roots::none();
            let project = RootName::new("project").ok().and_then(|name| {
                roots.add(name.clone(), &folder, Access::Write).ok()?;
                roots.folder(&name, Access::Write).ok()
            });
            if let Some(project) = project {
                paths = paths.with_project(project);
            }
        }
        Self {
            engine,
            job,
            control,
            environment: environment.name.clone(),
            env_position,
            base: Scope::new(env_value),
            posture,
            state: job
                .snapshot
                .as_ref()
                .map_or(RunState::Queued, |snapshot| snapshot.state),
            trace,
            pass: 0,
            next_pass: 0,
            begins: None,
            held: Vec::new(),
            next_serial: 0,
            paths,
            time_limit: None,
            replay: None,
            shadow_allowed: false,
            shadowed: Vec::new(),
            loaded,
        }
    }

    /// The addresses a context prepared for `operation` may reach: the
    /// target's surfaces at this environment, and what the operation's own
    /// permission manifest adds. Never another operation's.
    fn destinations_of(&self, operation: &OperationName) -> Destinations {
        let plan = &self.loaded.plan;
        let environment = &plan.environments[self.env_position];
        let mut addresses: Vec<String> = plan
            .surfaces
            .iter()
            .enumerate()
            .map(|(position, surface)| address_of(environment, position, &surface.address))
            .collect();
        if let Some(found) = plan
            .operations
            .iter()
            .find(|found| &found.name == operation)
        {
            addresses.extend(found.manifest.destinations.iter().cloned());
        }
        Destinations::new(addresses)
    }

    /// Tells the control plane the run has moved, and remembers it.
    pub async fn moved(&mut self, to: RunState) -> Result<(), Failure> {
        if self.state == to {
            return Ok(());
        }
        self.control.send(EngineEvent::Moved(to)).await?;
        self.state = to;
        Ok(())
    }

    /// Sends an event the run does not depend on.
    pub async fn tell(&self, event: EngineEvent) {
        // An event of this kind is not waited for: if it cannot be stored
        // the run is not harmed, and the control plane logs its own loss.
        let _ = self.control.send(event).await;
    }

    /// The checks made before every operation call: the directive, the
    /// lease, and the run's own time limit.
    pub fn checkpoint(&self, needed: Duration) -> Result<(), Stop> {
        match self.job.lease.directive() {
            Directive::Cancel => return Err(Stop::Cancelled),
            Directive::Pause => return Err(Stop::LeaseEnding),
            Directive::Continue => {}
        }
        if !self.job.lease.leaves(needed) {
            return Err(Stop::LeaseEnding);
        }
        if self
            .time_limit
            .is_some_and(|limit| self.engine.deps.clock.elapsed() >= limit)
        {
            return Err(Stop::Fail(codes::fail(
                FailureClass::Busy,
                codes::RUN_TIME_EXCEEDED,
                "The run passed the longest time its pipeline allows.",
                "Resume the run; what it verified stays verified.",
            )));
        }
        Ok(())
    }

    fn surface_ref(&self, surface: SurfaceIndex) -> Result<SurfaceRef, Failure> {
        let plan = &self.loaded.plan;
        let found = plan.surface(surface).ok_or_else(|| {
            codes::job_invalid("A binding names a surface the plan does not hold.")
        })?;
        let environment = &plan.environments[self.env_position];
        let mut at_environment = found.clone();
        at_environment.address = address_of(environment, surface.get(), &found.address);
        Ok(SurfaceRef {
            target: plan.header.target.clone(),
            environment: environment.name.clone(),
            surface: at_environment,
        })
    }

    /// Choose: the bindings that qualify, in the plan's order, and those
    /// before the first of them that did not.
    pub fn candidates(
        &self,
        operation: &OperationPlan,
        scope: &Scope,
    ) -> Result<(Vec<Candidate>, Vec<PassedOver>), Failure> {
        let mut qualified = Vec::new();
        let mut passed_over: Vec<PassedOver> = Vec::new();
        let mut best: Option<Failure> = None;
        let mut pass_over = |mode: &Mode, failure: Failure, list: &mut Vec<PassedOver>| {
            list.push(PassedOver {
                mode: mode.clone(),
                class: failure.class(),
                code: failure.code().clone(),
            });
            if best
                .as_ref()
                .is_none_or(|held| reason_rank(failure.class()) < reason_rank(held.class()))
            {
                best = Some(failure);
            }
        };
        // A test or a trial names the binding it is evidence for: that one
        // runs, and no other (audit finding AC-6).
        let named = self
            .job
            .binding
            .as_ref()
            .filter(|pin| pin.operation == operation.name);
        if let Some(pin) = named
            && !pin.holds_in(&self.loaded.plan, operation)
        {
            return Err(crate::binding::changed(pin));
        }
        for (position, binding) in operation.bindings.iter().enumerate() {
            if self
                .job
                .pin
                .as_ref()
                .is_some_and(|pin| pin != &binding.mode)
                || named.is_some_and(|pin| pin.position != position)
            {
                continue;
            }
            // Against a replay nothing acts, so a binding not yet verified
            // may be tested there: an offline test is how a draft or a
            // shadow binding earns its promotion (decision 0180). A retired
            // one never runs.
            let qualifies = if self.replay.is_some() {
                binding.status != BindingStatus::Retired
            } else {
                status_qualifies(binding.status, self.posture)
            };
            if !qualifies {
                let why = format!(
                    "The binding of `{}` through the mode `{}` is `{}` and may not act.",
                    operation.name, binding.mode, binding.status
                );
                pass_over(&binding.mode, no_binding(why), &mut passed_over);
                continue;
            }
            let handle = match self.admits(operation, binding, scope)? {
                Ok(handle) => handle,
                Err(failure) => {
                    pass_over(&binding.mode, failure, &mut passed_over);
                    continue;
                }
            };
            let reason = if self.job.pin.is_some() || named.is_some() {
                ChoiceReason::Pinned
            } else if !qualified.is_empty() {
                ChoiceReason::FellBack
            } else if passed_over.is_empty() {
                ChoiceReason::FirstInOrder
            } else {
                ChoiceReason::EarlierDidNotQualify
            };
            qualified.push(Candidate {
                binding: position,
                reason,
                handle,
            });
        }
        if qualified.is_empty() {
            return Err(best.unwrap_or_else(|| {
                codes::fail(
                    FailureClass::Busy,
                    codes::NO_BINDING,
                    format!(
                        "The operation `{}` has no binding that may act.",
                        operation.name
                    ),
                    "Verify a binding of the operation and publish the target again.",
                )
            }));
        }
        Ok((qualified, passed_over))
    }

    /// Whether a binding may act now, whatever its status: the registry
    /// has a driver ready for its mode, its surface has neither drifted nor
    /// been blocked at the edge, its session is usable, and its
    /// precondition holds for the call's inputs. `Ok(Err(..))` says why it
    /// may not; `Err` is a failure of the call itself.
    fn admits(
        &self,
        operation: &OperationPlan,
        binding: &BindingPlan,
        scope: &Scope,
    ) -> Result<Result<DriverHandle, Failure>, Failure> {
        let conditions = &self.job.conditions;
        let handle = match &self.replay {
            Some(replay) => self
                .engine
                .deps
                .registry
                .replay_driver(&binding.mode, Arc::clone(replay)),
            None => self.engine.deps.registry.driver(&binding.mode),
        };
        let handle = match handle {
            Ok(handle) => handle,
            Err(failure) => {
                return Ok(Err(no_binding(format!(
                    "No driver is ready for the mode `{}` ({}).",
                    binding.mode,
                    failure.code()
                ))));
            }
        };
        let surface_ref = self.surface_ref(binding.surface)?;
        let surface_name = &surface_ref.surface.name;
        if let DriverHealth::Degraded(code) | DriverHealth::Unavailable(code) =
            handle.health(&surface_ref)
        {
            return Ok(Err(no_binding(format!(
                "The driver of the mode `{}` is not ready ({code}).",
                binding.mode
            ))));
        }
        if conditions
            .sessions
            .iter()
            .any(|(surface, state)| surface == surface_name && !session_usable(*state))
        {
            return Ok(Err(codes::fail(
                FailureClass::SessionExpired,
                codes::NEEDS_LOGIN,
                format!("The session of the surface `{surface_name}` has ended."),
                "Sign in again with `anything2mcp session login`, then resume the run.",
            )));
        }
        if conditions.edge_blocked.contains(surface_name) {
            return Ok(Err(codes::fail(
                FailureClass::EdgeBlocked,
                codes::EDGE_BLOCKED,
                format!("The surface `{surface_name}` is blocked at the edge."),
                "Wait for the block to end; the engine does not go around it.",
            )));
        }
        if conditions
            .drifted
            .iter()
            .any(|(name, mode)| name == &operation.name && mode == &binding.mode)
        {
            return Ok(Err(codes::fail(
                FailureClass::Drift,
                codes::BINDING_DRIFTED,
                format!(
                    "The binding of `{}` through the mode `{}` has drifted.",
                    operation.name, binding.mode
                ),
                "Repair the binding, verify it, and publish the target again.",
            )));
        }
        if let Some(precondition) = binding.precondition
            && !self.loaded.holds(precondition, scope)?
        {
            return Ok(Err(no_binding(format!(
                "The precondition of the binding through the mode `{}` does not hold.",
                binding.mode
            ))));
        }
        Ok(Ok(handle))
    }

    /// The operation's bindings in `shadow`, in the plan's order, each with
    /// the candidate it runs as or why it may not run now (decision 0207).
    /// Only a live job that runs against the system has any: never a try,
    /// a replay, or a job that pins a mode or names a binding.
    pub fn shadow_candidates(
        &self,
        operation: &OperationPlan,
        scope: &Scope,
    ) -> Result<Vec<ShadowCandidate>, Failure> {
        if self.replay.is_some()
            || self.posture != Posture::Live
            || self.job.pin.is_some()
            || self.job.binding.is_some()
        {
            return Ok(Vec::new());
        }
        let mut found = Vec::new();
        for (position, binding) in operation.bindings.iter().enumerate() {
            if binding.status != BindingStatus::Shadow {
                continue;
            }
            let candidate = self
                .admits(operation, binding, scope)?
                .map(|handle| Candidate {
                    binding: position,
                    reason: ChoiceReason::FirstInOrder,
                    handle,
                });
            found.push((position, candidate));
        }
        Ok(found)
    }

    /// Tells the control plane which binding was chosen and why.
    pub async fn chosen(
        &self,
        operation: &OperationName,
        candidate: &Candidate,
        passed_over: &[PassedOver],
    ) {
        let choice = candidate.handle.choice();
        self.tell(EngineEvent::BindingChosen {
            operation: operation.clone(),
            mode: candidate.handle.mode().clone(),
            reason: candidate.reason,
            provider: choice.provider.to_string(),
            provider_version: choice.version,
            provider_digest: choice.digest,
            passed_over: passed_over.to_vec(),
        })
        .await;
    }

    /// The posture a context is prepared in. In the sandbox every context
    /// is prepared so that the driver's own read-only guard is on and the
    /// handle refuses every step that commits; the one exception is the
    /// context in which a declared test record is written, and the caller
    /// has checked the record's key before it asks for that one.
    fn posture_for(&self, writes: bool) -> Posture {
        match (self.posture, writes) {
            (Posture::Sandbox { .. }, writes) => Posture::Sandbox {
                test_records: writes,
            },
            (posture, _) => posture,
        }
    }

    /// Prepare: the session lease, then the driver's context. A context
    /// already held for the same mode and surface, in the same posture, is
    /// used again. `writes` says the context is for writing a record; it
    /// changes the posture only in the sandbox.
    pub async fn hold(
        &mut self,
        operation: &OperationName,
        binding: &BindingPlan,
        candidate: &Candidate,
        again: bool,
        writes: bool,
    ) -> Result<usize, Failure> {
        self.hold_on(
            operation,
            &binding.mode,
            binding.surface,
            candidate,
            again,
            writes,
        )
        .await
    }

    /// The same, for a mode on a surface with no binding: what a discovery
    /// job prepares, through the surface's own mode.
    pub async fn hold_on(
        &mut self,
        operation: &OperationName,
        mode: &Mode,
        surface: SurfaceIndex,
        candidate: &Candidate,
        again: bool,
        writes: bool,
    ) -> Result<usize, Failure> {
        let posture = self.posture_for(writes);
        self.hold_at(operation, mode, surface, candidate, again, posture)
            .await
    }

    /// Prepares, as [`Run::hold_on`] does, in the posture given: for a
    /// binding in shadow, a posture in which the driver's own read-only
    /// guard is on and the handle refuses every step that commits.
    pub async fn hold_at(
        &mut self,
        operation: &OperationName,
        mode: &Mode,
        surface: SurfaceIndex,
        candidate: &Candidate,
        again: bool,
        posture: Posture,
    ) -> Result<usize, Failure> {
        let destinations = self.destinations_of(operation);
        if let Some(position) = self.held.iter().position(|held| {
            held.mode == *mode
                && held.surface == surface
                && held.posture == posture
                && held.destinations == destinations
                && !held.poisoned
        }) {
            self.begin_pass(operation, candidate.reason, Duration::ZERO, Duration::ZERO);
            return Ok(position);
        }
        let waited = self.engine.deps.clock.elapsed();
        let surface_ref = self.surface_ref(surface)?;
        // A replay uses no session: nothing reaches the system.
        let lease = if self.replay.is_none() && self.loaded.plan.auth_of(surface).is_some() {
            let want = SessionWant {
                surface: surface_ref.surface.name.clone(),
                headroom: None,
                again,
            };
            Some(self.control.session_lease(&want).await?)
        } else {
            None
        };
        let queue = self.engine.deps.clock.elapsed().saturating_sub(waited);
        let preparing = self.engine.deps.clock.elapsed();
        let supervisor = candidate.handle.supervisor(
            surface_ref
                .surface
                .programs
                .iter()
                .map(ProgramRef::from)
                .collect(),
        );
        let deadline = Deadline::after(self.engine.deps.settings.prepare_timeout);
        let context = PrepareContext {
            run: self.job.run,
            epoch: self.job.epoch,
            surface: &surface_ref,
            plan: &self.loaded.plan,
            posture,
            deadline,
            redact: self.engine.deps.redact.as_ref(),
            destinations: &destinations,
            paths: &self.paths,
            supervisor: &supervisor,
        };
        // It is safe to drop the future of `prepare`, so it runs under its
        // deadline.
        let prepared = tokio::time::timeout_at(
            deadline.at(),
            caught(candidate.handle.prepare(&context, lease.as_ref())),
        )
        .await;
        let prepared = match prepared {
            Ok(Some(Ok(prepared))) => prepared,
            Ok(Some(Err(failure))) => return Err(failure.failure),
            Ok(None) => return Err(panicked(mode, "preparing")),
            Err(_) => {
                return Err(codes::fail(
                    FailureClass::Transient,
                    codes::PREPARE_TIMED_OUT,
                    format!(
                        "The driver of the mode `{}` was not ready within its time limit.",
                        mode
                    ),
                    "Call the tool again.",
                ));
            }
        };
        let prepare = self.engine.deps.clock.elapsed().saturating_sub(preparing);
        self.begin_pass(operation, candidate.reason, queue, prepare);
        let serial = self.next_serial;
        self.next_serial += 1;
        self.held.push(Held {
            mode: mode.clone(),
            surface,
            destinations,
            surface_ref,
            handle: candidate.handle.clone(),
            prepared,
            lease,
            posture,
            serial,
            poisoned: false,
        });
        Ok(self.held.len() - 1)
    }

    fn begin_pass(
        &mut self,
        operation: &OperationName,
        reason: ChoiceReason,
        queue: Duration,
        prepare: Duration,
    ) {
        self.pass = self.next_pass;
        self.next_pass += 1;
        self.begins = Some(PassInfo {
            operation: operation.clone(),
            reason,
            queue,
            prepare,
        });
    }

    /// The time limit of a step: its own, or the settings' when the plan
    /// states none.
    pub fn limit_of(&self, step: &Step) -> Duration {
        if step.timeout_ms == 0 {
            self.engine.deps.settings.step_timeout
        } else {
            Duration::from_millis(u64::from(step.timeout_ms))
        }
    }

    /// Whether "absent" from these steps, in the held context's mode,
    /// proves that a record is not there: the mode's manifest states every
    /// one of their kinds as authoritative for absence (decision 0185).
    pub fn proves_absence(&self, held: usize, steps: &[Step]) -> bool {
        self.held[held].handle.proves_absence(steps)
    }

    /// The time a list of steps may take, each at its time limit.
    pub fn time_of<'s>(&self, steps: impl IntoIterator<Item = &'s Step>) -> Duration {
        steps.into_iter().map(|step| self.limit_of(step)).sum()
    }

    /// Asks for a rate budget for the steps of a pass, when the surface
    /// declares a limit.
    pub async fn budget(&self, held: usize, steps: usize) -> Result<(), Failure> {
        let surface = &self.held[held].surface_ref.surface;
        if surface.rate.is_none() || steps == 0 {
            return Ok(());
        }
        let mut wanted = u32::try_from(steps).unwrap_or(u32::MAX);
        while wanted > 0 {
            let granted = self.control.rate(&surface.name, wanted).await?;
            if granted == 0 {
                return Err(codes::fail(
                    FailureClass::Busy,
                    codes::NO_RATE_BUDGET,
                    format!(
                        "No rate budget was granted for the surface `{}`.",
                        surface.name
                    ),
                    "Call the tool again later.",
                ));
            }
            wanted = wanted.saturating_sub(granted);
        }
        Ok(())
    }

    /// Takes the trace and the count of passes out of the run, so that what
    /// a binding in shadow does leaves no step in the run's trace and moves
    /// no pass of it: the trace's signature is the live call's alone.
    pub fn set_aside(&mut self) -> Aside {
        Aside {
            trace: self.trace.take(),
            pass: self.pass,
            next_pass: self.next_pass,
            begins: self.begins.take(),
        }
    }

    /// Puts back what [`Run::set_aside`] took.
    pub fn restore(&mut self, aside: Aside) {
        self.trace = aside.trace;
        self.pass = aside.pass;
        self.next_pass = aside.next_pass;
        self.begins = aside.begins;
    }

    /// Marks every held context of this posture as not to be used again:
    /// after a binding in shadow was cancelled, whatever step it was in.
    pub fn poison_posture(&mut self, posture: Posture) {
        for held in &mut self.held {
            if held.posture == posture {
                held.poisoned = true;
            }
        }
    }

    /// Marks a held context as not to be used again, after a failure.
    pub fn poison(&mut self, held: usize) {
        if let Some(held) = self.held.get_mut(held) {
            held.poisoned = true;
        }
    }

    /// Relays a sign of expiry on the lease a context was prepared under.
    pub async fn report_expiry(&mut self, held: usize) {
        if let Some(lease) = self.held.get(held).and_then(|held| held.lease.as_ref()) {
            self.control.report_expiry(lease, ExpirySign::Refused).await;
        }
        self.poison(held);
    }

    /// Act: one step, through the handle. A step is carried out as written;
    /// a `transient` failure is tried again, at most as often as the
    /// settings say, and only when nothing it sent can have been a write.
    ///
    /// `Ok(None)` when the step's `when` is false.
    pub async fn step(&mut self, cx: StepCx<'_>) -> Result<Option<StepOutcome>, StepFailure> {
        let loaded = Arc::clone(&self.loaded);
        if let Some(when) = cx.step.when
            && !loaded
                .holds(when, cx.scope)
                .map_err(StepFailure::not_sent)?
        {
            return Ok(None);
        }
        let mut tries = 0;
        loop {
            // A retry starts the step again from evaluation.
            let mut args = Values::new();
            for arg in &cx.step.args {
                let value = loaded
                    .eval(arg.value, cx.scope)
                    .map_err(StepFailure::not_sent)?;
                args.insert(arg.name.clone(), value);
            }
            if args.iter().any(|(_, value)| masked(value)) {
                // The mask stands for a value the redactor removed. Sent
                // to the system it would be looked up, or written, as if
                // it were that value.
                return Err(StepFailure::not_sent(codes::fail(
                    FailureClass::UnknownVariant,
                    codes::ARGUMENT_MASKED,
                    format!(
                        "An argument of a `{}` step holds the mask of a value the redactor removed, and a mask is never sent to the system.",
                        cx.step.kind
                    ),
                    "The system showed a registered secret where the step takes a value; change the step so that it does not take that value.",
                )));
            }
            let limit = self.limit_of(cx.step);
            let deadline = Deadline::after(limit);
            let input = StepInput {
                args: &args,
                record: cx.record.map(|record| record.record),
                key: cx.record.and_then(|record| record.key),
                stamp: cx.record.and_then(|record| record.stamp),
                deadline,
            };
            let empty = Values::new();
            let recorded = cx.record.map_or(&empty, |record| record.recorded);
            // The control plane's answerer, for this step and its record
            // alone, of whether another record of the operation holds a
            // value (decision 0193); with none, every question answers
            // `CannotTell`.
            let others: Arc<dyn a2m_provider::OtherRecords> = cx
                .record
                .as_ref()
                .and_then(|record| self.control.other_records(record.operation, record.record))
                .unwrap_or_else(|| Arc::new(CannotTell));
            let started = self.engine.deps.clock.elapsed();
            let held = &mut self.held[cx.held];
            let result = if cx.step.commits {
                // The future of a committing step is never dropped, so it
                // runs to its end whatever the deadline says. The deadline
                // is the driver's to keep.
                let authority = cx.token.map_or(Authority::Read, Authority::Write);
                let acting = caught(held.handle.act_asking(
                    &mut held.prepared,
                    cx.step,
                    &input,
                    authority,
                    recorded,
                    others,
                ));
                match acting.await {
                    Some(result) => result,
                    None => {
                        // Nothing says whether the write left before the
                        // panic: the record is in doubt.
                        held.poisoned = true;
                        Err(StepFailure::unknown(codes::fail(
                            FailureClass::PossibleCommit,
                            codes::NO_CONFIRMATION,
                            format!(
                                "The driver of the mode `{}` panicked in a `{}` step that commits.",
                                held.mode, cx.step.kind
                            ),
                            "Reconcile the run: read the record back before anything is written again.",
                        )))
                    }
                }
            } else {
                let acting = caught(held.handle.act_asking(
                    &mut held.prepared,
                    cx.step,
                    &input,
                    Authority::Read,
                    recorded,
                    others,
                ));
                match tokio::time::timeout_at(deadline.at(), acting).await {
                    Ok(Some(result)) => result,
                    Ok(None) => {
                        held.poisoned = true;
                        Err(StepFailure::unknown(panicked(
                            &held.mode,
                            cx.step.kind.as_str(),
                        )))
                    }
                    Err(_) => {
                        held.poisoned = true;
                        Err(StepFailure::unknown(Failure::new(
                            cx.step.fails_as.class,
                            cx.step.fails_as.code.clone(),
                            format!(
                                "A `{}` step did not end within its time limit.",
                                cx.step.kind
                            ),
                            "Call the tool again; if it repeats, check the system and the step's time limit.",
                        )))
                    }
                }
            };
            let took = self.engine.deps.clock.elapsed().saturating_sub(started);
            // A trial tells its control plane what each step that only
            // reads observed, for a fixture to be recorded from it
            // (decision 0196). Nothing that commits, and no other job.
            if self.job.kind == JobKind::Try && !cx.step.commits {
                match &result {
                    Ok(StepOutcome::Done(observed)) => {
                        self.control.step_observed(cx.step, Some(&observed.values));
                    }
                    Ok(StepOutcome::Absent(_)) => self.control.step_observed(cx.step, None),
                    Err(_) => {}
                }
            }
            let artifacts = self.keep_captures(&cx, &result).await;
            self.traced(&cx, &result, took, artifacts);
            match result {
                Err(failure)
                    if failure.failure.class() == FailureClass::Transient
                        && tries < self.engine.deps.settings.transient_retries
                        && (!cx.step.commits
                            || ((failure.sent == Sent::NotSent
                                || (cx.step.idempotent && failure.sent == Sent::Answered))
                                // A save is sent again only while the lease
                                // covers it.
                                && self.job.lease.leaves(limit + cx.then))) =>
                {
                    tries += 1;
                }
                other => return other.map(Some),
            }
        }
    }

    /// Keeps what a step captured, as the run's capture level says: each
    /// capture passes the redactor and the trace's own check, is kept as
    /// evidence through `Control`, and is named in the step's trace record.
    /// This happens before the step after it is sent, so what was captured
    /// before a committing step is kept before the save. A capture that
    /// cannot be kept is dropped and the run goes on. The capture that says
    /// how the step was done goes into the trace record, not here.
    async fn keep_captures(
        &mut self,
        cx: &StepCx<'_>,
        result: &Result<StepOutcome, StepFailure>,
    ) -> Vec<ArtifactRef> {
        let Ok(StepOutcome::Done(observed) | StepOutcome::Absent(observed)) = result else {
            return Vec::new();
        };
        let mut kept = Vec::new();
        for capture in &observed.captures {
            if capture.is_acting() {
                continue;
            }
            let Some(trace) = self.trace.as_mut() else {
                break;
            };
            let kind = if capture.media_type.starts_with("image/") {
                ArtifactKind::Screenshot
            } else {
                ArtifactKind::Exchange
            };
            // Bytes that are text are redacted once more; an image cannot
            // be, and arrives with no report of what was masked, so the
            // trace's own check drops it.
            let redact = &self.engine.deps.redact;
            let bytes = match std::str::from_utf8(&capture.bytes) {
                Ok(text) => redact.redact(text).into_bytes(),
                Err(_) => capture.bytes.clone(),
            };
            let ArtifactOutcome::Kept(artifact) = trace.artifact(OfferedArtifact {
                kind,
                at: CapturePoint::Step,
                bytes,
                masked: None,
            }) else {
                continue;
            };
            let keeping = KeptCapture {
                record: cx.record.map(|record| record.record),
                pass: self.pass,
                phase: cx.phase,
                step: u32::try_from(cx.position).unwrap_or(u32::MAX),
                name: redact.redact(&capture.name),
                media_type: capture.media_type.clone(),
                bytes: artifact.bytes,
            };
            if let Ok(blob) = self.control.keep_capture(keeping).await {
                kept.push(ArtifactRef {
                    kind: artifact.kind,
                    at: artifact.at,
                    blob: blob.digest(),
                });
            }
        }
        kept
    }

    /// Writes the record of one step into the trace.
    fn traced(
        &mut self,
        cx: &StepCx<'_>,
        result: &Result<StepOutcome, StepFailure>,
        took: Duration,
        artifacts: Vec<ArtifactRef>,
    ) {
        let begins = self.begins.take().map(|info| {
            let held = &self.held[cx.held];
            PassStart {
                operation: info.operation,
                mode: held.mode.clone(),
                reason: info.reason,
                driver: held.handle.choice().digest,
                queue: info.queue,
                prepare: info.prepare,
            }
        });
        let Some(trace) = self.trace.as_mut() else {
            return;
        };
        let mut values = Vec::new();
        if let Ok(StepOutcome::Done(observed) | StepOutcome::Absent(observed)) = result {
            for field in cx.fields {
                if let Some(raw) = observed.values.get(&field.name) {
                    let normalized = self
                        .loaded
                        .normalize(Some(field.field_type), raw)
                        .unwrap_or_else(|_| raw.clone());
                    values.push(TracedValue::read(
                        field.name.clone(),
                        field.sensitivity,
                        raw.clone(),
                        normalized,
                    ));
                }
            }
        }
        let acting = match result {
            Ok(StepOutcome::Done(observed) | StepOutcome::Absent(observed)) => observed.acting(),
            Err(_) => None,
        };
        let outcome = match result {
            Ok(_) => TraceOutcome::Done,
            Err(failure) => TraceOutcome::Failed {
                class: failure.failure.class(),
                code: failure.failure.code().clone(),
            },
        };
        trace.record(StepRecord {
            pass: self.pass,
            phase: cx.phase,
            step: u32::try_from(cx.position).unwrap_or(u32::MAX),
            kind: cx.step.kind.clone(),
            record: cx.record.map(|record| record.ledger_key.clone()),
            begins,
            took,
            locating: None,
            acting,
            values,
            outcome,
            artifacts,
        });
    }

    /// Runs a list of steps that commit nothing, merging what they observe
    /// into the scope's `result`. `Ok(None)` when a lookup found nothing.
    pub async fn read_steps(
        &mut self,
        held: usize,
        phase: Phase,
        steps: &[Step],
        scope: &mut Scope,
        record: Option<RecordCx<'_>>,
        fields: &[Field],
    ) -> Result<Option<Observed>, StepFailure> {
        let mut all = Observed::default();
        for (position, step) in steps.iter().enumerate() {
            if step.commits {
                return Err(StepFailure::not_sent(codes::fail(
                    FailureClass::Policy,
                    codes::SANDBOX_READ_ONLY,
                    format!(
                        "A `{}` step that commits stands where only reading is allowed.",
                        step.kind
                    ),
                    "Compile the target again; a read never holds a step that commits.",
                )));
            }
            let outcome = self
                .step(StepCx {
                    held,
                    phase,
                    position,
                    step,
                    scope,
                    record,
                    token: None,
                    fields,
                    then: Duration::ZERO,
                })
                .await?;
            match outcome {
                None => {}
                Some(StepOutcome::Absent(_)) => return Ok(None),
                Some(StepOutcome::Done(observed)) => merge(scope, &mut all, observed),
            }
        }
        Ok(Some(all))
    }

    /// Gives every held driver context back.
    pub async fn release(&mut self) {
        for held in self.held.drain(..) {
            let _ = caught(held.handle.release(held.prepared, held.poisoned)).await;
        }
    }

    /// Takes a held context out of the job's hands, for the sandbox port.
    /// The run's checked paths: the project folder of the request that
    /// started it, checked as a root, or none.
    pub(crate) fn paths(&self) -> &CheckedPaths {
        &self.paths
    }

    pub fn take(&mut self, held: usize) -> Prepared {
        self.held.remove(held).prepared
    }

    /// The surface at a position, at this run's environment.
    pub fn surface_at(&self, surface: SurfaceIndex) -> Result<SurfaceRef, Failure> {
        self.surface_ref(surface)
    }

    /// Which of the job's contexts a held context is. A driver may keep
    /// what it read of a record with its context, so a record read in one
    /// context is not known to the context prepared in its place.
    pub fn serial_of(&self, held: usize) -> u64 {
        self.held[held].serial
    }

    /// The mode of a held context.
    pub fn mode_of(&self, held: usize) -> Mode {
        self.held[held].mode.clone()
    }

    /// The surface of a held context.
    pub fn surface_of(&self, held: usize) -> a2m_core::SurfaceName {
        self.held[held].surface_ref.surface.name.clone()
    }

    /// Whether a job in this state of the kernel may write.
    pub fn may_write(&self) -> Result<(), Failure> {
        match &self.job.conditions.health {
            KernelHealth::Ok => Ok(()),
            KernelHealth::Degraded(cause) => Err(codes::fail(
                FailureClass::Busy,
                codes::ENGINE_DEGRADED,
                format!("The engine takes reads only for now ({cause})."),
                "See `anything2mcp engine status`, and call the tool again when it is ready.",
            )),
        }
    }
}

/// Merges what a step observed into what the pass has observed so far, and
/// into the scope's `result` for the steps after it.
pub(crate) fn merge(scope: &mut Scope, all: &mut Observed, observed: Observed) {
    if let Value::Record(result) = &mut scope.result {
        for (name, value) in observed.values.iter() {
            result.insert(name.to_owned(), value.clone());
        }
    }
    for (name, value) in observed.values {
        all.values.insert(name, value);
    }
    for (name, value) in observed.for_reconcile {
        all.for_reconcile.insert(name, value);
    }
    if observed.assigned_key.is_some() {
        all.assigned_key = observed.assigned_key;
    }
    // A step's captures were kept, as the run's capture level says, when
    // the step ended; nothing of them is held beyond it.
    drop(observed.captures);
}

/// Whether a value holds the redaction mark: it was masked by the redactor,
/// by the engine's own when it was stored or by the handle's when a driver
/// returned it.
pub(crate) fn masked(value: &Value) -> bool {
    match value {
        Value::Text(text) => text.contains(REDACTED),
        Value::List(items) => items.iter().any(masked),
        Value::Record(values) => values.iter().any(|(_, value)| masked(value)),
        _ => false,
    }
}

/// The failure of a step, or of preparing, in which a provider's code
/// panicked.
fn panicked(mode: &Mode, doing: &str) -> Failure {
    codes::fail(
        FailureClass::UnknownVariant,
        codes::DRIVER_PANICKED,
        format!("The driver of the mode `{mode}` panicked in `{doing}`."),
        "Report this to the provider's author; the engine went on without the step.",
    )
}

fn session_usable(state: SessionState) -> bool {
    matches!(
        state,
        SessionState::Valid | SessionState::Suspect | SessionState::Renewing
    )
}

fn address_of(environment: &a2m_core::EnvironmentPlan, surface: usize, default: &str) -> String {
    environment
        .addresses
        .iter()
        .find(|address| address.surface.get() == surface)
        .map_or_else(|| default.to_owned(), |address| address.address.clone())
}

/// Finds the operation a pipeline step or a tool names.
pub(crate) fn operation(loaded: &Loaded, index: OperationIndex) -> Result<&OperationPlan, Failure> {
    loaded
        .plan
        .operation(index)
        .ok_or_else(|| codes::job_invalid("The tool names an operation the plan does not hold."))
}

/// The answerer of a step that has no record, or whose control plane gives
/// none: it cannot tell.
struct CannotTell;

impl a2m_provider::OtherRecords for CannotTell {
    fn holding<'a>(
        &'a self,
        _name: &'a str,
        _value: a2m_core::Digest,
    ) -> a2m_provider::BoxFuture<'a, a2m_provider::RecordedElsewhere> {
        Box::pin(async { a2m_provider::RecordedElsewhere::CannotTell })
    }
}
