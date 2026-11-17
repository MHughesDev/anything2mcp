//! One job in hand: the plan it runs, the drivers it has prepared, its
//! trace, and the parts of the loop every operation call goes through:
//! choose, prepare, and one step at a time through the driver's handle.

use std::sync::Arc;
use std::time::Duration;

use a2m_auth::{ExpirySign, SessionLease};
use a2m_core::{
    BindingPlan, BindingStatus, Environment, Failure, FailureClass, Mode, OperationIndex,
    OperationName, OperationPlan, RecordKey, RunState, SessionState, Step, SurfaceIndex, Value,
    Values,
};
use a2m_ledger::{IntentToken, RecordRef};
use a2m_provider::{
    Authority, CheckedPaths, Deadline, Destinations, DriverHandle, DriverHealth, Observed, Posture,
    PrepareContext, Prepared, ProgramRef, Sent, StepFailure, StepInput, StepOutcome, SurfaceRef,
};
use a2m_telemetry::{
    ChoiceReason, PassStart, Phase, StepRecord, TraceBuilder, TraceOutcome, TracedValue,
};
use tokio::time::Instant;

use crate::codes;
use crate::compare::Field;
use crate::control::{Control, EngineEvent, PassedOver, SessionWant};
use crate::engine::Engine;
use crate::job::{Directive, JobSpec, KernelHealth};
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
    surface_ref: SurfaceRef,
    handle: DriverHandle,
    prepared: Prepared,
    lease: Option<SessionLease>,
    poisoned: bool,
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
    pub key: &'s RecordKey,
    pub stamp: &'s str,
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
    begins: Option<PassInfo>,
    held: Vec<Held>,
    destinations: Destinations,
    paths: CheckedPaths,
    /// The moment the run's own time limit passes.
    pub time_limit: Option<Instant>,
}

/// Whether a binding of this status may be chosen in this posture.
fn status_qualifies(status: BindingStatus, posture: Posture) -> bool {
    match posture {
        Posture::Live => status == BindingStatus::Verified,
        Posture::Sandbox { .. } | Posture::Discovery => status != BindingStatus::Retired,
    }
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
        let mut addresses: Vec<String> = Vec::new();
        for (position, surface) in plan.surfaces.iter().enumerate() {
            addresses.push(address_of(environment, position, &surface.address));
        }
        for operation in &plan.operations {
            addresses.extend(operation.manifest.destinations.iter().cloned());
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
            begins: None,
            held: Vec::new(),
            destinations: Destinations::new(addresses),
            paths: CheckedPaths::none(),
            time_limit: None,
            loaded,
        }
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
        if self.time_limit.is_some_and(|limit| Instant::now() >= limit) {
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
        let conditions = &self.job.conditions;
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
        for (position, binding) in operation.bindings.iter().enumerate() {
            if self.job.pin.as_ref().is_some_and(|pin| pin != &binding.mode) {
                continue;
            }
            let no_binding = |why: String| {
                codes::fail(
                    FailureClass::Busy,
                    codes::NO_BINDING,
                    why,
                    "Check the binding's status and its provider with `anything2mcp engine status`.",
                )
            };
            if !status_qualifies(binding.status, self.posture) {
                let why = format!(
                    "The binding of `{}` through the mode `{}` is `{}` and may not act.",
                    operation.name, binding.mode, binding.status
                );
                pass_over(&binding.mode, no_binding(why), &mut passed_over);
                continue;
            }
            let handle = match self.engine.deps.registry.driver(&binding.mode) {
                Ok(handle) => handle,
                Err(failure) => {
                    let why = format!(
                        "No driver is ready for the mode `{}` ({}).",
                        binding.mode,
                        failure.code()
                    );
                    pass_over(&binding.mode, no_binding(why), &mut passed_over);
                    continue;
                }
            };
            let surface_ref = self.surface_ref(binding.surface)?;
            let surface_name = &surface_ref.surface.name;
            if let DriverHealth::Degraded(code) | DriverHealth::Unavailable(code) =
                handle.health(&surface_ref)
            {
                let why = format!(
                    "The driver of the mode `{}` is not ready ({code}).",
                    binding.mode
                );
                pass_over(&binding.mode, no_binding(why), &mut passed_over);
                continue;
            }
            if conditions
                .sessions
                .iter()
                .any(|(surface, state)| surface == surface_name && !session_usable(*state))
            {
                let failure = codes::fail(
                    FailureClass::SessionExpired,
                    codes::NEEDS_LOGIN,
                    format!("The session of the surface `{surface_name}` has ended."),
                    "Sign in again with `anything2mcp session login`, then resume the run.",
                );
                pass_over(&binding.mode, failure, &mut passed_over);
                continue;
            }
            if conditions.edge_blocked.contains(surface_name) {
                let failure = codes::fail(
                    FailureClass::EdgeBlocked,
                    codes::EDGE_BLOCKED,
                    format!("The surface `{surface_name}` is blocked at the edge."),
                    "Wait for the block to end; the engine does not go around it.",
                );
                pass_over(&binding.mode, failure, &mut passed_over);
                continue;
            }
            if conditions
                .drifted
                .iter()
                .any(|(name, mode)| name == &operation.name && mode == &binding.mode)
            {
                let failure = codes::fail(
                    FailureClass::Drift,
                    codes::BINDING_DRIFTED,
                    format!(
                        "The binding of `{}` through the mode `{}` has drifted.",
                        operation.name, binding.mode
                    ),
                    "Repair the binding, verify it, and publish the target again.",
                );
                pass_over(&binding.mode, failure, &mut passed_over);
                continue;
            }
            if let Some(precondition) = binding.precondition
                && !self.loaded.holds(precondition, scope)?
            {
                let why = format!(
                    "The precondition of the binding through the mode `{}` does not hold.",
                    binding.mode
                );
                pass_over(&binding.mode, no_binding(why), &mut passed_over);
                continue;
            }
            let reason = if self.job.pin.is_some() {
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
                    format!("The operation `{}` has no binding that may act.", operation.name),
                    "Verify a binding of the operation and publish the target again.",
                )
            }));
        }
        Ok((qualified, passed_over))
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

    /// Prepare: the session lease, then the driver's context. A context
    /// already held for the same mode and surface is used again.
    pub async fn hold(
        &mut self,
        operation: &OperationName,
        binding: &BindingPlan,
        candidate: &Candidate,
        again: bool,
    ) -> Result<usize, Failure> {
        if let Some(position) = self.held.iter().position(|held| {
            held.mode == binding.mode && held.surface == binding.surface && !held.poisoned
        }) {
            self.begin_pass(operation, candidate.reason, Duration::ZERO, Duration::ZERO);
            return Ok(position);
        }
        let waited = self.engine.deps.clock.elapsed();
        let surface_ref = self.surface_ref(binding.surface)?;
        let lease = if self.loaded.plan.auth_of(binding.surface).is_some() {
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
            posture: self.posture,
            deadline,
            redact: self.engine.deps.redact.as_ref(),
            destinations: &self.destinations,
            paths: &self.paths,
            supervisor: &supervisor,
        };
        // It is safe to drop the future of `prepare`, so it runs under its
        // deadline.
        let prepared = tokio::time::timeout_at(
            deadline.at(),
            candidate.handle.prepare(&context, lease.as_ref()),
        )
        .await;
        let prepared = match prepared {
            Ok(Ok(prepared)) => prepared,
            Ok(Err(failure)) => return Err(failure.failure),
            Err(_) => {
                return Err(codes::fail(
                    FailureClass::Transient,
                    codes::PREPARE_TIMED_OUT,
                    format!(
                        "The driver of the mode `{}` was not ready within its time limit.",
                        binding.mode
                    ),
                    "Call the tool again.",
                ));
            }
        };
        let prepare = self.engine.deps.clock.elapsed().saturating_sub(preparing);
        self.begin_pass(operation, candidate.reason, queue, prepare);
        self.held.push(Held {
            mode: binding.mode.clone(),
            surface: binding.surface,
            surface_ref,
            handle: candidate.handle.clone(),
            prepared,
            lease,
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
        if self.begins.is_some() || self.pass > 0 || self.trace_has_records {
            self.pass += 1;
        }
        self.trace_has_records = true;
        self.begins = Some(PassInfo {
            operation: operation.clone(),
            reason,
            queue,
            prepare,
        });
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
                    codes::NO_BUDGET,
                    format!("No rate budget was granted for the surface `{}`.", surface.name),
                    "Call the tool again later.",
                ));
            }
            wanted = wanted.saturating_sub(granted);
        }
        Ok(())
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
            && !loaded.holds(when, cx.scope).map_err(StepFailure::not_sent)?
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
            let limit = if cx.step.timeout_ms == 0 {
                self.engine.deps.settings.step_timeout
            } else {
                Duration::from_millis(u64::from(cx.step.timeout_ms))
            };
            let deadline = Deadline::after(limit);
            let input = StepInput {
                args: &args,
                record: cx.record.map(|record| record.record),
                key: cx.record.map(|record| record.key),
                stamp: cx.record.map(|record| record.stamp),
                deadline,
            };
            let started = self.engine.deps.clock.elapsed();
            let held = &mut self.held[cx.held];
            let result = if cx.step.commits {
                // The future of a committing step is never dropped, so it
                // runs to its end whatever the deadline says.
                let authority = cx.token.map_or(Authority::Read, Authority::Write);
                held.handle
                    .act(&mut held.prepared, cx.step, &input, authority)
                    .await
            } else {
                let acting = held
                    .handle
                    .act(&mut held.prepared, cx.step, &input, Authority::Read);
                match tokio::time::timeout_at(deadline.at(), acting).await {
                    Ok(result) => result,
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
            self.traced(&cx, &result, took);
            match result {
                Err(failure)
                    if failure.failure.class() == FailureClass::Transient
                        && tries < self.engine.deps.settings.transient_retries
                        && (!cx.step.commits
                            || failure.sent == Sent::NotSent
                            || (cx.step.idempotent && failure.sent == Sent::Answered)) =>
                {
                    tries += 1;
                }
                other => return other.map(Some),
            }
        }
    }

    /// Writes the record of one step into the trace.
    fn traced(&mut self, cx: &StepCx<'_>, result: &Result<StepOutcome, StepFailure>, took: Duration) {
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
            record: cx.record.map(|record| record.key.clone()),
            begins,
            took,
            locating: None,
            acting: None,
            values,
            outcome,
            artifacts: Vec::new(),
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
            let _ = held.handle.release(held.prepared, held.poisoned).await;
        }
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
    all.captures.extend(observed.captures);
}

fn session_usable(state: SessionState) -> bool {
    matches!(
        state,
        SessionState::Valid | SessionState::Suspect | SessionState::Renewing
    )
}

fn address_of(
    environment: &a2m_core::EnvironmentPlan,
    surface: usize,
    default: &str,
) -> String {
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
