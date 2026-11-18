//! `Control` in one process: everything the engine asks for while it runs a
//! job, answered by the store, the policy engine, the session keeper, and
//! the scheduler directly.
//!
//! One `InProcessControl` is built for one claimed job. It attaches the run
//! and the lease number to every call itself, so the engine can neither
//! forge nor forget them.
//!
//! The engine gives up a call that does not answer in time and drops its
//! future, so every call here stands or falls as one transaction of the
//! store. A transaction that was handed to the store commits or not as a
//! whole, whether or not anyone still waits for it. What follows a commit
//! in a call is only a notice in memory, that a lock may be free or that a
//! run changed, and it is given by a guard as the call ends, however it
//! ends. Two calls keep a blob before they record it, a result and a
//! trace: dropped in between, they leave a blob nothing names, and the call
//! made again stores the same blob under the same name.

use std::sync::Arc;
use std::time::Duration;

use a2m_auth::{AuthEntry, ExpirySign, LeaseRequest, Purpose, SessionKey, SessionLease, Wait};
use a2m_core::{
    Digest, Failure, FailureClass, Identity, Plan, RunState, SurfaceIndex, SurfaceName, ToolRuns,
    Values,
};
use a2m_engine::{
    BoxFuture, Clearance, Control, EngineEvent, Evidence, GuardQuestion, KeptCapture, Preview,
    SessionWant, ShadowCompared, encode_values,
};
use a2m_ledger::{
    Acted, ActedToken, BlobKind, BlobRef, Durability, EventKind, Fence, IntentToken, LockHeld,
    NewBlob, ReadBack, Reconciled, RecordIntent, RecordRef, RetentionClass, RunRow, Settled,
    ShadowComparison,
};
use a2m_policy::{Context, LimitCheck, ModuleName, Outcome, Question, Resource, ToolRef};
use a2m_sched::{BudgetKey, LeaseRef, RateLimit, ResourceKey};
use a2m_telemetry::Phase;
use a2m_vault::OwnerRef;
use serde_json::json;

use crate::codes;
use crate::kernel::Kernel;
use crate::runner::CallerFacts;

/// How long a job waits for a record another run holds.
const LOCK_WAIT: Duration = Duration::from_secs(60);

/// The keeper's entry for a surface's sign-in, field by field from the
/// plan (decision 0121). `session.login` builds the same entry from the
/// same plan, so a sign-in a person makes is the session a run leases.
pub(crate) fn entry_of(auth: &a2m_core::AuthPlan) -> AuthEntry {
    let mut entry = AuthEntry::new(auth.provider.clone());
    for setting in &auth.settings {
        entry = entry.setting(&setting.name, &setting.value);
    }
    for credential in &auth.credentials {
        entry = entry.credential(&credential.role, credential.reference.clone());
    }
    entry.idle = auth.idle_ms.map(Duration::from_millis);
    entry.absolute = auth.absolute_ms.map(Duration::from_millis);
    entry.attempt_limit = auth.attempt_limit;
    entry.movable = auth.movable;
    entry
}

/// `Control` for one job of one run, in the process that holds the store.
pub(crate) struct InProcessControl {
    kernel: Kernel,
    fence: Fence,
    lease: LeaseRef,
    run: RunRow,
    plan: Arc<Plan>,
    caller: CallerFacts,
}

impl InProcessControl {
    pub(crate) fn new(
        kernel: Kernel,
        fence: Fence,
        lease: LeaseRef,
        run: RunRow,
        plan: Arc<Plan>,
        caller: CallerFacts,
    ) -> Self {
        Self {
            kernel,
            fence,
            lease,
            run,
            plan,
            caller,
        }
    }

    fn redacted(&self, failure: Failure) -> Failure {
        self.kernel.redact().redact_failure(&failure)
    }

    /// The identity the run acts as: the person who started it. A dedicated
    /// account comes with toolsets, which are not built.
    fn identity(&self) -> Identity {
        Identity::Person(self.run.principal)
    }

    fn surface_index(&self, name: &SurfaceName) -> Option<SurfaceIndex> {
        self.plan
            .surfaces
            .iter()
            .position(|surface| &surface.name == name)
            .and_then(|at| u32::try_from(at).ok())
            .map(SurfaceIndex::new)
    }

    /// The key of the lock on one record, as the scheduler queues waits
    /// for it.
    async fn lock_key(&self, record: RecordRef) -> Result<ResourceKey, Failure> {
        let row = self
            .kernel
            .store()?
            .read(move |tx| tx.ledger().record(record))
            .await?;
        Ok(ResourceKey::record(
            self.run.workspace,
            &self.run.target,
            &self.run.environment,
            &row.key_hash,
        ))
    }

    /// A guard that, when it is dropped, tells the scheduler that the
    /// record's lock may be free, so that the run that waited first asks
    /// the ledger again, and tells whoever follows a run that one changed.
    /// The key is read before the write the guard is made for, so that the
    /// notice needs no further wait and is given even when the call's
    /// future is dropped while its transaction commits. A notice for a
    /// lock that was not released costs a waiter one more look.
    async fn notice(&self, record: RecordRef) -> Notice {
        Notice {
            kernel: self.kernel.clone(),
            key: self.lock_key(record).await.ok(),
        }
    }
}

/// See [`InProcessControl::notice`].
struct Notice {
    kernel: Kernel,
    key: Option<ResourceKey>,
}

impl Drop for Notice {
    fn drop(&mut self) {
        if let (Some(key), Ok(sched)) = (&self.key, self.kernel.sched()) {
            sched.lock_freed(key);
        }
        self.kernel.run_changed();
    }
}

impl Control for InProcessControl {
    fn plan<'a>(&'a self, hash: &'a Digest) -> BoxFuture<'a, Result<Arc<Plan>, Failure>> {
        Box::pin(async move {
            self.kernel
                .plan_by_hash(&self.run.target, hash)
                .await
                .map_err(|failure| self.redacted(failure))
        })
    }

    fn decide<'a>(&'a self, question: &'a GuardQuestion) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move {
            let policy = self.kernel.policy().cloned().ok_or_else(|| {
                codes::busy(
                    codes::ENGINE_DEGRADED,
                    "No policy set is loaded, so nothing can be decided.",
                    "Call `doctor.run` for the fix.",
                )
            })?;
            let module = self
                .plan
                .tool(&question.tool)
                .and_then(|tool| tool.module.clone())
                .unwrap_or_else(|| "main".to_owned());
            let mut context = Context::plain(self.run.workspace, question.environment.clone());
            context.effect = question.effect;
            context.hazard = question.hazard;
            context.records = question.records;
            context.toolset = self.caller.toolset.clone();
            context.caller_is_agent = self.caller.is_agent;
            context.acting_for = self.caller.acting_for.clone();
            let request = a2m_policy::Request {
                principal: self.caller.principal.clone(),
                question: Question::UseTool,
                resource: Resource::Tool(ToolRef {
                    target: self.run.target.clone(),
                    module: ModuleName::new(&module)?,
                    tool: question.tool.clone(),
                }),
                context,
                run: Some(self.run.id),
                diff_hash: None,
            };
            let limits = LimitCheck {
                principal: self.caller.principal.clone(),
                workspace: self.run.workspace,
                records: question.records,
                count_call: false,
            };
            // The decision is recorded whether it allows or refuses, so the
            // transaction commits either way.
            let decided = self
                .kernel
                .store()?
                .write(Durability::Synced, move |tx| {
                    let decision = policy.decide(tx, &request);
                    if let Some(refusal) = decision.refusal.clone() {
                        return Ok(Err(refusal));
                    }
                    if !decision.allowed() {
                        return Ok(Err(codes::policy(
                            a2m_policy::NOT_GRANTED,
                            "Policy did not allow this call.",
                            "Ask an administrator for a grant on this tool.",
                        )));
                    }
                    Ok(policy.check_limits(tx, &limits))
                })
                .await?;
            decided.map_err(|failure| self.redacted(failure))
        })
    }

    fn session_lease<'a>(
        &'a self,
        want: &'a SessionWant,
    ) -> BoxFuture<'a, Result<SessionLease, Failure>> {
        Box::pin(async move {
            let keeper = self.kernel.keeper()?;
            let auth = self
                .surface_index(&want.surface)
                .and_then(|index| self.plan.auth_of(index))
                .ok_or_else(|| {
                    codes::input(
                        a2m_auth::codes::AUTH_SETTINGS_INVALID,
                        format!("The surface `{}` declares no sign-in.", want.surface),
                        "Declare the surface in the target's auth file, then publish again.",
                    )
                })?;
            let entry = entry_of(auth);

            let settings = self.kernel.settings();
            let wait = if settings.text("engine.person") == Some("absent") {
                Wait::NoWait
            } else {
                let minutes = settings.int("sessions.login_wait_minutes").unwrap_or(5);
                match u64::try_from(minutes) {
                    Ok(0) | Err(_) => Wait::NoWait,
                    Ok(minutes) => Wait::Interactive(Duration::from_secs(minutes * 60)),
                }
            };
            let decided = self.run.decision.unwrap_or(self.caller.decision);
            keeper
                .lease(LeaseRequest {
                    key: SessionKey {
                        workspace: self.run.workspace,
                        target: self.run.target.clone(),
                        environment: self.run.environment.clone(),
                        identity: self.identity(),
                    },
                    entry: Arc::new(entry),
                    run: self.run.id,
                    decided,
                    acting_for: self.caller.person(),
                    wait,
                    headroom: want.headroom,
                    purpose: Purpose::Run,
                })
                .await
                .map_err(|failure| self.redacted(failure))
        })
    }

    fn report_expiry<'a>(&'a self, lease: &'a SessionLease, sign: ExpirySign) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if let Ok(keeper) = self.kernel.keeper() {
                keeper.report(lease, sign).await;
            }
        })
    }

    fn rate<'a>(
        &'a self,
        surface: &'a SurfaceName,
        want: u32,
    ) -> BoxFuture<'a, Result<u32, Failure>> {
        Box::pin(async move {
            let sched = self.kernel.sched()?;
            let key = BudgetKey::surface(
                self.run.workspace,
                &self.run.target,
                &self.run.environment,
                surface.as_str(),
            )?;
            // The surface's own declared budget, set once; a surface that
            // declares none takes the scheduler's conservative default.
            if let Some(rate) = self
                .surface_index(surface)
                .and_then(|index| self.plan.surface(index))
                .and_then(|surface| surface.rate)
                && !self.kernel.budget_noted(&key)
            {
                sched
                    .set_budget(
                        key.clone(),
                        RateLimit {
                            capacity: rate.requests.max(1),
                            refill: rate.requests.max(1),
                            per: Duration::from_millis(u64::from(rate.per_ms.max(1))),
                        },
                    )
                    .await?;
                // Noted only once it is set: a call dropped before that
                // sets it the next time.
                self.kernel.note_budget(&key);
            }
            sched
                .rate(&self.lease, &[key], want.max(1))
                .await
                .map(|grant| grant.requests)
                .map_err(|failure| self.redacted(failure))
        })
    }

    fn store_preview<'a>(&'a self, preview: &'a Preview) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move {
            let fence = self.fence;
            let preview = preview.clone();
            self.kernel
                .store()?
                .write(Durability::Synced, move |tx| {
                    let mut ledger = tx.ledger();
                    ledger.plan_records(&fence, &preview.records)?;
                    ledger.store_diff(&fence, preview.diff, Some(preview.diff_blob))
                })
                .await
                .map_err(|failure| self.redacted(failure))
                .inspect(|()| self.kernel.run_changed())
        })
    }

    fn approval<'a>(&'a self, diff: &'a Digest) -> BoxFuture<'a, Result<Clearance, Failure>> {
        Box::pin(async move {
            let policy = self.kernel.policy().cloned().ok_or_else(|| {
                codes::busy(
                    codes::ENGINE_DEGRADED,
                    "No policy set is loaded, so no approval can be checked.",
                    "Call `doctor.run` for the fix.",
                )
            })?;
            let fence = self.fence;
            let run = self.run.id;
            let diff = *diff;
            // Unknown is off: only the setting read as true lets a
            // pre-approval begin a write.
            let pre_approval =
                self.kernel.settings().flag(crate::bounds::PRE_APPROVAL) == Some(true);
            let module = self
                .plan
                .tool(&self.run.tool_name)
                .and_then(|tool| tool.module.clone())
                .unwrap_or_else(|| "main".to_owned());
            // The tool's hazard, from the plan: a hazard forbids writing
            // without a person, so the question must carry it. A tool the
            // plan does not hold is asked about with no record count, and
            // no pre-approval rule covers a run of unknown size.
            let hazard = self
                .plan
                .tool(&self.run.tool_name)
                .and_then(|tool| match tool.runs {
                    ToolRuns::Operation(index) => {
                        self.plan.operation(index).map(|found| found.hazard)
                    }
                    ToolRuns::Pipeline(index) => {
                        self.plan.pipeline(index).map(|found| found.hazard)
                    }
                });
            let mut context = Context::plain(self.run.workspace, self.run.environment.clone());
            context.effect = self.run.effect;
            context.hazard = hazard.unwrap_or(a2m_core::Hazard::None);
            context.toolset = self.caller.toolset.clone();
            context.caller_is_agent = self.caller.is_agent;
            context.acting_for = self.caller.acting_for.clone();
            let known = hazard.is_some();
            // The test records the run's environment declares, when it is a
            // test environment: the policy engine is told whether every
            // record of the run is one of them (decision 0145).
            let test_keys: Option<Vec<a2m_core::RecordKey>> = self
                .plan
                .environments
                .iter()
                .find(|environment| environment.name == self.run.environment)
                .filter(|environment| environment.kind == a2m_core::EnvironmentKind::Test)
                .map(|environment| environment.test_records.clone());
            let mut request = a2m_policy::Request {
                principal: self.caller.principal.clone(),
                question: Question::WriteWithoutPerson,
                resource: Resource::Tool(ToolRef {
                    target: self.run.target.clone(),
                    module: ModuleName::new(&module)?,
                    tool: self.run.tool_name.clone(),
                }),
                context,
                run: Some(run),
                diff_hash: Some(diff),
            };
            // One transaction: the question "may this write proceed without
            // a person" is decided and recorded; then either that decision
            // or a person's approval, marked used here, clears the run, and
            // the ledger moves it to `running`. A run that nobody cleared
            // waits, and the recorded decision still commits.
            let cleared = self
                .kernel
                .store()?
                .write(Durability::Synced, move |tx| {
                    // How many records the run would write, as the ledger
                    // holds them: a pre-approval rule covers a run only up
                    // to its own record count.
                    let records = tx.ledger().records(run)?;
                    if known {
                        request.context.records = u32::try_from(records.len()).ok();
                    }
                    request.context.test_records = test_keys.as_ref().is_some_and(|keys| {
                        !records.is_empty() && records.iter().all(|row| keys.contains(&row.key))
                    });
                    let decision = policy.decide(tx, &request);
                    // A pre-approval begins a write only where the
                    // installation lets one: with `policy.pre_approval` off
                    // every write waits for a person, whatever rule the
                    // set in force holds.
                    let pre_approved = decision.clearance();
                    if pre_approval && let Some(clearance) = pre_approved {
                        tx.ledger().begin_writes(&fence, clearance)?;
                        return Ok(Ok(Clearance::Cleared));
                    }
                    // A refusal stops the run here; it is not a wait. That
                    // is a kill switch, and it is a policy engine that
                    // could not decide: a decider that cannot decide never
                    // leaves the write to an approval (fail closed).
                    if let Outcome::Refused(refusal) = decision.outcome() {
                        return Ok(Err(refusal.clone()));
                    }
                    match policy.consume(tx, run, &diff) {
                        Ok(clearance) => {
                            tx.ledger().begin_writes(&fence, clearance)?;
                            Ok(Ok(Clearance::Cleared))
                        }
                        Err(missing) if missing.code() == &a2m_policy::APPROVAL_MISSING => {
                            // The refusal to write without a person is a
                            // recorded decision; the run's own events name
                            // it, so that it can be explained later.
                            tx.ledger().append_event(
                                &fence,
                                EventKind::Need,
                                &json!({
                                    "needs": "approval",
                                    "decision": decision.id.to_string(),
                                    "diff": diff.to_string(),
                                    // The policy engine keeps the setting
                                    // too, so with it off no rule applies.
                                    "pre_approval": if pre_approved.is_some() || !pre_approval {
                                        "off by the setting `policy.pre_approval`"
                                    } else {
                                        "none applies"
                                    },
                                }),
                                None,
                            )?;
                            Ok(Ok(Clearance::Waits))
                        }
                        Err(stale) => Ok(Err(stale)),
                    }
                })
                .await?;
            if matches!(cleared, Ok(Clearance::Cleared)) {
                // The approval is used: the run no longer waits to be
                // resumed.
                self.kernel.inner.signals.forget(run);
            }
            self.kernel.run_changed();
            cleared.map_err(|failure| self.redacted(failure))
        })
    }

    fn lock<'a>(&'a self, record: RecordRef) -> BoxFuture<'a, Result<LockHeld, Failure>> {
        Box::pin(async move {
            let store = self.kernel.store()?;
            let sched = self.kernel.sched()?;
            let key = self.lock_key(record).await?;
            let fence = self.fence;
            loop {
                // The ticket is taken before the ledger is asked, so that a
                // release between the refusal and the wait is not missed.
                let ticket = sched.lock_ticket(&key);
                match store
                    .write(Durability::Synced, move |tx| {
                        tx.ledger().take_lock(&fence, record)
                    })
                    .await
                {
                    Ok(held) => return Ok(held),
                    Err(locked) if locked.code() == &a2m_ledger::codes::RECORD_LOCKED => {
                        sched
                            .wait_lock(&self.lease, ticket, LOCK_WAIT)
                            .await
                            .map_err(|failure| self.redacted(failure))?;
                    }
                    Err(other) => return Err(self.redacted(other)),
                }
            }
        })
    }

    fn intend<'a>(
        &'a self,
        intents: Vec<RecordIntent>,
    ) -> BoxFuture<'a, Result<Vec<IntentToken>, Failure>> {
        Box::pin(async move {
            // Fail closed: no ledger, no write. The intents are durable
            // before a token exists.
            self.kernel
                .store()?
                .store_intents(&self.fence, intents)
                .await
                .map_err(|failure| self.redacted(failure))
                .inspect(|_| self.kernel.run_changed())
        })
    }

    fn acted<'a>(
        &'a self,
        token: IntentToken,
        acted: Acted,
    ) -> BoxFuture<'a, Result<ActedToken, Failure>> {
        Box::pin(async move {
            self.kernel
                .store()?
                .write(Durability::Synced, move |tx| {
                    tx.ledger().mark_acted(token, acted)
                })
                .await
                .map_err(|failure| self.redacted(failure))
                .inspect(|_| self.kernel.run_changed())
        })
    }

    fn verified<'a>(
        &'a self,
        token: &'a ActedToken,
        read_back: ReadBack,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move {
            // The ledger releases the record's lock in the transaction
            // that marks it verified; a run waiting for it is told as this
            // call ends.
            let _notice = self.notice(token.record()).await;
            self.kernel
                .store()?
                .store_verified(token, read_back)
                .await
                .map_err(|failure| self.redacted(failure))
        })
    }

    fn keep_capture<'a>(&'a self, capture: KeptCapture) -> BoxFuture<'a, Result<BlobRef, Failure>> {
        Box::pin(async move {
            // A step's capture is evidence of the run, of the short class:
            // a blob in the evidence space, sealed for the run's person,
            // kept, as traces are: nothing is deleted yet, and the setting
            // `evidence.trace_retention_days` is not built (decision 0236).
            // Its bytes passed the redactor and the run's capture level in
            // the engine; the name is redacted again here, as everything
            // stored is.
            let kind = if capture.media_type.starts_with("image/") {
                BlobKind::Screenshot
            } else {
                BlobKind::Exchange
            };
            let redact = self.kernel.redact();
            let blob = self
                .evidence(Evidence {
                    kind,
                    class: RetentionClass::Short,
                    bytes: capture.bytes,
                })
                .await?;
            // What ties it to the run: an event of the run, durable before
            // this call returns, so that a capture taken before a
            // committing step is in the store before that step is sent.
            let payload = json!({
                "capture": redact.redact(&capture.name),
                "media_type": redact.redact(&capture.media_type),
                "pass": capture.pass,
                "phase": match capture.phase {
                    Phase::Act => "act",
                    Phase::Confirm => "confirm",
                    Phase::ReadBack => "read_back",
                },
                "position": capture.step,
                "step": capture.record.map(|record| record.step_seq),
                "record": capture.record.map(|record| record.record_seq),
                "digest": blob.digest().to_string(),
            });
            let fence = self.fence;
            self.kernel
                .store()?
                .write(Durability::Synced, move |tx| {
                    tx.ledger()
                        .append_event(&fence, EventKind::Need, &payload, Some(blob))
                        .map(|_| ())
                })
                .await
                .map_err(|failure| self.redacted(failure))?;
            Ok(blob)
        })
    }

    fn project_folder(&self) -> Option<std::path::PathBuf> {
        self.caller.project.clone()
    }

    fn step_observed(&self, step: &a2m_core::Step, observed: Option<&a2m_core::Values>) {
        // Kept only for a trial a fixture is being recorded from.
        crate::fixture::observed(&self.kernel, self.run.input_hash, step, observed);
    }

    fn other_records(
        &self,
        operation: &a2m_core::OperationName,
        asking: RecordRef,
    ) -> Option<Arc<dyn a2m_provider::OtherRecords>> {
        let store = self.kernel.store().ok()?.clone();
        Some(Arc::new(StoredOthers {
            store,
            workspace: self.run.workspace,
            principal: self.run.principal,
            operation: operation.clone(),
            asking,
        }))
    }

    fn acknowledged<'a>(
        &'a self,
        token: &'a ActedToken,
        evidence: Option<BlobRef>,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move {
            // The ledger releases the record's lock in the transaction that
            // ends it; a run waiting for it is told as this call ends.
            let _notice = self.notice(token.record()).await;
            // The ledger requires the answer's evidence: where the engine
            // could not keep it, the record stays in doubt (decision 0172).
            let evidence = evidence.ok_or_else(|| {
                Failure::new(
                    FailureClass::Busy,
                    a2m_engine::codes::ACKNOWLEDGEMENT_NOT_KEPT,
                    "The system's answer could not be kept, so the record cannot end on it and stays in doubt.",
                    "Look at the system; the run stops until the record is settled.",
                )
            })?;
            self.kernel
                .store()?
                .store_acknowledged(token, evidence)
                .await
                .map_err(|failure| self.redacted(failure))
        })
    }

    fn failed<'a>(
        &'a self,
        record: RecordRef,
        why: &'a Failure,
        evidence: Option<BlobRef>,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move {
            let fence = self.fence;
            let why = self.redacted(why.clone());
            // A record fails from `planned` as well, when the record it
            // depends on failed: it holds no lock then, and the notice
            // costs nothing.
            let _notice = self.notice(record).await;
            self.kernel
                .store()?
                .write(Durability::Synced, move |tx| {
                    tx.ledger().mark_failed(&fence, record, &why, evidence)
                })
                .await
                .map_err(|failure| self.redacted(failure))
        })
    }

    fn settle<'a>(
        &'a self,
        record: RecordRef,
        seen: Reconciled,
    ) -> BoxFuture<'a, Result<Settled, Failure>> {
        Box::pin(async move {
            let fence = self.fence;
            let _notice = self.notice(record).await;
            self.kernel
                .store()?
                .write(Durability::Synced, move |tx| {
                    tx.ledger().settle(&fence, record, seen)
                })
                .await
                .map_err(|failure| self.redacted(failure))
        })
    }

    fn send<'a>(&'a self, event: EngineEvent) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move {
            let store = self.kernel.store()?;
            let fence = self.fence;
            let run = self.run.id;
            let sent = match event {
                EngineEvent::Moved(to) => {
                    store
                        .write(Durability::Synced, move |tx| {
                            let mut ledger = tx.ledger();
                            // The engine says `validating` before it has
                            // the plan, and may say a state the run is
                            // already in: that is no move.
                            if ledger.run(run)?.state == to {
                                return Ok(());
                            }
                            ledger.set_state(&fence, to, None)
                        })
                        .await
                }
                EngineEvent::StepStarted {
                    step_seq,
                    operation,
                } => {
                    let payload = json!({"step": step_seq, "operation": operation.as_str()});
                    store
                        .write(Durability::Relaxed, move |tx| {
                            tx.ledger()
                                .append_event(&fence, EventKind::StepStarted, &payload, None)
                                .map(|_| ())
                        })
                        .await
                }
                EngineEvent::BindingChosen {
                    operation,
                    mode,
                    provider,
                    provider_version,
                    provider_digest,
                    passed_over,
                    ..
                } => {
                    let payload = json!({
                        "operation": operation.as_str(),
                        "mode": mode.as_str(),
                        "provider": provider,
                        "provider_version": provider_version.to_string(),
                        "provider_digest": provider_digest.to_string(),
                        "passed_over": passed_over.iter().map(|passed| json!({
                            "mode": passed.mode.as_str(),
                            "class": passed.class.as_str(),
                            "code": passed.code.as_str(),
                        })).collect::<Vec<_>>(),
                    });
                    store
                        .write(Durability::Relaxed, move |tx| {
                            tx.ledger()
                                .append_event(&fence, EventKind::BindingChosen, &payload, None)
                                .map(|_| ())
                        })
                        .await
                }
                EngineEvent::BeforeCommit { record, outputs } => {
                    let payload = json!({
                        "before_commit": true,
                        "step": record.step_seq,
                        "record": record.record_seq,
                    });
                    store
                        .write(Durability::Synced, move |tx| {
                            tx.ledger()
                                .append_event(&fence, EventKind::Need, &payload, Some(outputs))
                                .map(|_| ())
                        })
                        .await
                }
                EngineEvent::Result(values) => self.store_result(&values).await,
                EngineEvent::BindingHealth {
                    operation,
                    mode,
                    surface,
                    failure,
                } => {
                    // The health of a binding is the engine's own table,
                    // which is not built; the sign is kept as a run event.
                    let payload = json!({
                        "binding_health": true,
                        "operation": operation.as_str(),
                        "mode": mode.as_str(),
                        "surface": surface.as_str(),
                        "class": failure.as_ref().map(|(class, _)| class.as_str()),
                        "code": failure.as_ref().map(|(_, code)| code.as_str()),
                    });
                    store
                        .write(Durability::Relaxed, move |tx| {
                            tx.ledger()
                                .append_event(&fence, EventKind::Need, &payload, None)
                                .map(|_| ())
                        })
                        .await
                }
                EngineEvent::VerifiedDiffers { evidence, .. } => {
                    store
                        .write(Durability::Synced, move |tx| {
                            tx.ledger().late_evidence(run, evidence).map(|_| ())
                        })
                        .await
                }
                EngineEvent::Discovered {
                    surface,
                    provider,
                    discoverer,
                    findings,
                    report,
                } => {
                    // Each finding was checked against its provider's
                    // manifest by the sandbox; the store names it by the
                    // digest of its content. What the discoverer reported,
                    // and which findings were its, is kept beside it for
                    // the scout (decision 0170).
                    let workspace = self.run.workspace;
                    let target = self.run.target.clone();
                    let run = self.run.id;
                    let ids: Vec<String> = findings
                        .iter()
                        .map(|finding| finding.id().to_string())
                        .collect();
                    let ended = crate::handlers::discovery_report_json(&report);
                    store
                        .write(Durability::Synced, move |tx| {
                            for finding in &findings {
                                tx.findings().put(&workspace, &target, finding)?;
                            }
                            let now = tx.now().as_unix_millis();
                            tx.tables(a2m_ledger::Owner::Actions).execute(
                                "INSERT INTO actions_discovery_results
                                    (run_id, provider, discoverer, surface, findings, report, sent_at)
                                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                                 ON CONFLICT (run_id, provider, discoverer)
                                 DO UPDATE SET findings = excluded.findings,
                                     report = excluded.report, sent_at = excluded.sent_at",
                                &[
                                    a2m_ledger::SqlValue::from(run.to_string()),
                                    a2m_ledger::SqlValue::from(provider.as_str()),
                                    a2m_ledger::SqlValue::from(discoverer.as_str()),
                                    a2m_ledger::SqlValue::from(surface.as_str()),
                                    a2m_ledger::SqlValue::from(json!(ids).to_string()),
                                    a2m_ledger::SqlValue::from(ended.to_string()),
                                    a2m_ledger::SqlValue::from(now),
                                ],
                            )?;
                            Ok(())
                        })
                        .await
                }
                EngineEvent::Trace(trace) => {
                    let blob = self
                        .evidence(Evidence {
                            kind: BlobKind::Trace,
                            class: RetentionClass::Long,
                            bytes: trace.bytes,
                        })
                        .await?;
                    store
                        .write(Durability::Relaxed, move |tx| {
                            tx.ledger().store_trace(run, blob)
                        })
                        .await
                }
            };
            self.kernel.run_changed();
            sent.map_err(|failure| self.redacted(failure))
        })
    }

    fn evidence<'a>(&'a self, blob: Evidence) -> BoxFuture<'a, Result<BlobRef, Failure>> {
        Box::pin(async move {
            self.kernel
                .store()?
                .blobs()
                .put(NewBlob {
                    workspace: self.run.workspace,
                    owner: OwnerRef::of(&self.identity()),
                    kind: blob.kind,
                    class: blob.class,
                    plain: blob.bytes,
                })
                .await
                .map_err(|failure| self.redacted(failure))
        })
    }

    /// The ledger keeps how a binding in shadow compared (decision 0207).
    /// The engine asks only for a live read that is not a try, a test, or a
    /// pinned call; the ledger refuses anything else on its own.
    fn keeps_shadow(&self) -> bool {
        true
    }

    fn shadow<'a>(&'a self, compared: ShadowCompared) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move {
            let comparison = ShadowComparison {
                fence: self.fence,
                operation: compared.operation,
                binding: compared.shadow.digest,
                mode: compared.shadow.mode,
                position: u32::try_from(compared.shadow.position).unwrap_or(u32::MAX),
                against: compared.verified.digest,
                writes: compared.writes,
                outcome: compared.outcome,
            };
            self.kernel
                .store()?
                .write(Durability::Synced, move |tx| {
                    tx.shadow().record(&comparison).map(|_| ())
                })
                .await
                .map_err(|failure| self.redacted(failure))
        })
    }
}

impl InProcessControl {
    async fn store_result(&self, values: &Values) -> Result<(), Failure> {
        // Everything leaving passes the redactor; the stored result is what
        // a later `run.get` returns.
        let redacted = self.kernel.redact().redact_values(values);
        // A trial a fixture is recorded from keeps its result short: the
        // values are the system's, unscrubbed (audit finding FX-6).
        let recording = self
            .kernel
            .inner
            .recordings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains_key(&self.run.input_hash);
        let blob = self
            .evidence(Evidence {
                kind: BlobKind::Result,
                class: if recording {
                    RetentionClass::Short
                } else {
                    RetentionClass::Long
                },
                bytes: encode_values(&redacted)?,
            })
            .await?;
        let fence = self.fence;
        self.kernel
            .store()?
            .write(Durability::Synced, move |tx| {
                tx.ledger().store_result(&fence, blob)
            })
            .await
    }

    /// Applies a job's outcome to the run, under the job's lease.
    pub(crate) async fn apply(&self, to: RunState, why: Option<Failure>) -> Result<(), Failure> {
        let fence = self.fence;
        let run = self.run.id;
        let why = why.map(|why| self.redacted(why));
        let moved = self
            .kernel
            .store()?
            .write(Durability::Synced, move |tx| {
                let mut ledger = tx.ledger();
                let state = ledger.run(run)?.state;
                // A run that ended while the job was in hand, declined at
                // the desk or cancelled, stays as it ended.
                if state == to || state.is_final() {
                    return Ok(());
                }
                ledger.set_state(&fence, to, why.as_ref())
            })
            .await;
        self.kernel.run_changed();
        moved
    }
}

/// Whether a failure class means a write may have been sent.
pub(crate) const fn may_have_written(class: FailureClass) -> bool {
    matches!(class, FailureClass::PossibleCommit)
}

/// The answer to a driver's question whether another record of an
/// operation holds a value (decision 0196): the values recorded before the
/// commit point of every other record of the operation in the workspace,
/// as the ledger stored them, compared by [`a2m_provider::recorded_digest`].
/// It names the record that holds the value and never hands a value out.
/// Where any candidate's value under the name was masked by the redactor,
/// or a stored value cannot be read, it cannot tell. Only the attempt of
/// each record that may have landed counts (`Ledger::recorded_before_commit`,
/// audit finding OR-1). A record of another person's run is never named to
/// the driver: the contract's answer that names one is given only for the
/// asking person's own runs, and for another's the answer is that it cannot
/// tell, which a driver takes as "may be held", so that nothing shown to the
/// caller names another person's run. At most [`OTHERS_MOST`] stored values
/// are opened; beyond them it cannot tell.
struct StoredOthers {
    store: a2m_ledger::Store,
    workspace: a2m_core::WorkspaceId,
    /// The person whose run asks.
    principal: a2m_core::PrincipalId,
    operation: a2m_core::OperationName,
    asking: RecordRef,
}

/// The most stored values one question opens.
const OTHERS_MOST: usize = 4_096;

/// What a driver is told of a record that holds the value: the record, when
/// its run is the asking person's own; that it cannot tell, otherwise, so
/// that no other person's run or record is named (audit finding OR-1).
fn told(
    asking: a2m_core::PrincipalId,
    holder: a2m_core::PrincipalId,
    record: RecordRef,
) -> a2m_provider::RecordedElsewhere {
    if asking == holder {
        a2m_provider::RecordedElsewhere::Held(record)
    } else {
        a2m_provider::RecordedElsewhere::CannotTell
    }
}

impl a2m_provider::OtherRecords for StoredOthers {
    fn holding<'a>(
        &'a self,
        name: &'a str,
        value: Digest,
    ) -> BoxFuture<'a, a2m_provider::RecordedElsewhere> {
        use a2m_provider::RecordedElsewhere;
        Box::pin(async move {
            let (workspace, operation) = (self.workspace, self.operation.clone());
            let Ok(rows) = self
                .store
                .read(move |tx| tx.ledger().recorded_before_commit(&workspace, &operation))
                .await
            else {
                return RecordedElsewhere::CannotTell;
            };
            if rows.len() > OTHERS_MOST {
                return RecordedElsewhere::CannotTell;
            }
            let mut held = None;
            for (record, blob) in rows {
                if record == self.asking {
                    continue;
                }
                let Ok(opened) = self.store.blobs().get(&blob).await else {
                    return RecordedElsewhere::CannotTell;
                };
                let Ok(values) = a2m_engine::decode_values(opened.expose()) else {
                    return RecordedElsewhere::CannotTell;
                };
                let Some(found) = values.get(name) else {
                    continue;
                };
                // A masked value names nothing that can be compared: the
                // value asked about may be the one the mask stands for.
                if matches!(found, a2m_core::Value::Text(text) if text.contains(a2m_core::REDACTED))
                {
                    return RecordedElsewhere::CannotTell;
                }
                if held.is_none() && a2m_provider::recorded_digest(found) == value {
                    held = Some(record);
                }
            }
            let Some(record) = held else {
                return RecordedElsewhere::NotHeld;
            };
            let Ok(holder) = self.store.read(move |tx| tx.ledger().run(record.run)).await else {
                return RecordedElsewhere::CannotTell;
            };
            told(self.principal, holder.principal, record)
        })
    }
}

#[cfg(test)]
mod told_tests {
    use super::*;

    #[test]
    fn a_record_of_another_persons_run_is_never_named_to_a_driver() {
        let (one, two) = (
            a2m_core::PrincipalId::from_bytes([1; 16]),
            a2m_core::PrincipalId::from_bytes([2; 16]),
        );
        let record = RecordRef {
            run: a2m_core::RunId::from_bytes([3; 16]),
            step_seq: 0,
            record_seq: 0,
        };
        assert_eq!(
            told(one, one, record),
            a2m_provider::RecordedElsewhere::Held(record)
        );
        assert_eq!(
            told(one, two, record),
            a2m_provider::RecordedElsewhere::CannotTell
        );
    }
}
