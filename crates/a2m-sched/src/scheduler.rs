//! The scheduler: admission, the queue, leases, waits, and budgets, over the
//! store.
//!
//! Every decision is one transaction of the store, so two calls never see
//! half of each other's work, and everything the scheduler knows is found
//! again after a restart. What is kept in memory is only who waits.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use a2m_core::{Failure, JobId, RunId, RunnerId, Timestamp};
use a2m_ledger::{Durability, Store, Tx};
use a2m_logs::{Attr, Clock, LogsAndTraces};
use tokio::sync::{Notify, oneshot, watch};

use crate::budget::{self, Take};
use crate::failure;
use crate::tables::{self, Acquired, LeaseClock, between, plus};
use crate::types::{
    Admitted, BudgetKey, CancelOutcome, Enqueue, JobEnd, JobLease, JobRow, LeaseRef, PauseScope,
    RateGrant, RateLimit, Renewal, ResourceKey, Resumed, RunnerOffer, RunnerTerms, SchedSettings,
    Slots, Submission, SweepReport,
};
use crate::waits::{LockTicket, Waits};

/// Counts the runs admitted.
pub const ADMITTED_COUNT: &str = "a2m.sched.admitted";
/// Counts the submissions refused, by failure class and code.
pub const REFUSED_COUNT: &str = "a2m.sched.refused";
/// Counts the leases granted.
pub const LEASED_COUNT: &str = "a2m.sched.leased";
/// Counts the leases lost.
pub const LOST_COUNT: &str = "a2m.sched.lost";
/// The time a job waited between being queued and being leased.
pub const QUEUE_WAIT: &str = "a2m.sched.queue_wait";

struct Inner {
    store: Store,
    clock: Arc<dyn Clock>,
    /// The count every lease is measured on: the steady clock, from the
    /// time of day this scheduler was made.
    leases: LeaseClock,
    settings: SchedSettings,
    logs_and_traces: Option<LogsAndTraces>,
    /// Woken when a waiting claim may find work it did not find before.
    work: Notify,
    /// Held around every change to who holds an exclusive resource, so that
    /// a release and a new waiter never miss each other.
    gate: tokio::sync::Mutex<()>,
    waits: Mutex<Waits>,
    /// Counts the changes to what the scheduler waits for.
    due: watch::Sender<u64>,
    /// The leased reconciles a person resumed the run of: the reconcile
    /// each queues if it ends in doubt, or is lost, takes no growing wait.
    /// Read and changed only inside the store's write transactions, which
    /// its one writer runs one after another, so a resume and the end of
    /// the reconcile it found are always in one order. Kept in memory: the
    /// wake serves the call that asked, which an engine's restart ends too,
    /// and the next resume finds the waiting reconcile in the store.
    resumed: Arc<Mutex<BTreeSet<JobId>>>,
}

impl Inner {
    fn waits(&self) -> MutexGuard<'_, Waits> {
        self.waits.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn resumed_set(resumed: &Mutex<BTreeSet<JobId>>) -> MutexGuard<'_, BTreeSet<JobId>> {
    resumed.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The scheduler. Cheap to clone; every clone is the same scheduler.
#[derive(Clone)]
pub struct Scheduler {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for Scheduler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scheduler")
            .field("settings", &self.inner.settings)
            .finish_non_exhaustive()
    }
}

/// A wait for one moment on the store's clock. It ends when a sweep finds
/// that the moment has come.
struct Timer {
    inner: Arc<Inner>,
    key: Option<(i64, u64)>,
    fired: Option<oneshot::Receiver<()>>,
}

impl Timer {
    async fn fired(&mut self) {
        if let Some(fired) = &mut self.fired {
            // A sender that went away without firing ends the wait too.
            let _ = fired.await;
            self.fired = None;
            self.key = None;
        }
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        if let Some(key) = self.key.take() {
            self.inner.waits().remove_timer(key);
        }
    }
}

/// Tells the one task that sweeps when the scheduler's earliest deadline may
/// have changed, so that it reads [`Scheduler::next_due`] again.
#[derive(Debug, Clone)]
pub struct DueWatch(watch::Receiver<u64>);

impl DueWatch {
    /// Waits until a deadline was added or removed. `false` when the
    /// scheduler is gone.
    pub async fn changed(&mut self) -> bool {
        self.0.changed().await.is_ok()
    }
}

impl Scheduler {
    /// A scheduler over `store`. Its time is the store's clock: every
    /// deadline it keeps is a moment of that clock, so a restart finds them
    /// again.
    #[must_use]
    pub fn new(
        store: Store,
        settings: SchedSettings,
        logs_and_traces: Option<LogsAndTraces>,
    ) -> Self {
        let clock = store.clock();
        let leases = LeaseClock::new(Arc::clone(&clock));
        let (due, _) = watch::channel(0);
        Self {
            inner: Arc::new(Inner {
                store,
                clock,
                leases,
                settings,
                logs_and_traces,
                work: Notify::new(),
                gate: tokio::sync::Mutex::new(()),
                waits: Mutex::new(Waits::default()),
                due,
                resumed: Arc::new(Mutex::new(BTreeSet::new())),
            }),
        }
    }

    /// The settings the scheduler was made with.
    #[must_use]
    pub fn settings(&self) -> &SchedSettings {
        &self.inner.settings
    }

    pub(crate) fn now(&self) -> Timestamp {
        self.inner.clock.now()
    }

    /// The store, for the scheduler's other modules.
    pub(crate) fn store(&self) -> &Store {
        &self.inner.store
    }

    /// Tells a waiting claim and the sweeping task that something changed.
    pub(crate) fn wake(&self) {
        self.changed();
    }

    /// Something changed that a waiting claim or the sweeping task may care
    /// about.
    fn changed(&self) {
        self.inner.work.notify_waiters();
        self.inner
            .due
            .send_modify(|count| *count = count.wrapping_add(1));
    }

    fn count(&self, name: &'static str, attrs: &[Attr], by: usize) {
        if let Some(logs_and_traces) = &self.inner.logs_and_traces
            && by > 0
        {
            logs_and_traces.count(name, attrs, u64::try_from(by).unwrap_or(u64::MAX));
        }
    }

    fn timer(&self, at: Timestamp) -> Timer {
        if self.now() >= at {
            return Timer {
                inner: Arc::clone(&self.inner),
                key: None,
                fired: None,
            };
        }
        let (key, fired) = self.inner.waits().add_timer(at);
        self.inner
            .due
            .send_modify(|count| *count = count.wrapping_add(1));
        Timer {
            inner: Arc::clone(&self.inner),
            key: Some(key),
            fired: Some(fired),
        }
    }

    // ---- admission -------------------------------------------------------

    /// Admits a run and queues its first job, in one transaction: the health
    /// check, the holds, the run row with its duplicate check, the queue
    /// bound, the caller's cap, the runner check, the budgets, and the job
    /// row. A refusal leaves nothing behind.
    ///
    /// # Errors
    ///
    /// `busy` with `engine_degraded`, `scheduler_paused`, `queue_full`,
    /// `caller_queue_full`, `runner_unavailable`, or `rate_limited`, each
    /// with a next step; `input`/`schema_invalid` for a malformed
    /// submission; `busy` failures of the store.
    pub async fn submit(&self, submission: Submission) -> Result<Admitted, Failure> {
        let settings = self.inner.settings;
        let durability = if submission.job.writes {
            Durability::Synced
        } else {
            Durability::Relaxed
        };
        let admitted = self
            .inner
            .store
            .write(durability, move |tx| {
                tables::admit(tx, &settings, submission, 0, None)
            })
            .await;
        match &admitted {
            Ok(Admitted::New { .. }) => {
                self.count(ADMITTED_COUNT, &[], 1);
                self.changed();
            }
            Ok(Admitted::Existing { .. }) => {}
            Err(refused) => self.count(
                REFUSED_COUNT,
                &[
                    Attr::Class(refused.class()),
                    Attr::Code(refused.code().clone()),
                ],
                1,
            ),
        }
        admitted
    }

    /// Queues a further job for a run that exists and has none waiting or
    /// leased: the execute job after an approval, or a resume. The same
    /// checks as [`Scheduler::submit`], the run row aside. It does what
    /// [`Scheduler::resume`] does, and answers a reconcile it found as a
    /// refusal.
    ///
    /// # Errors
    ///
    /// As [`Scheduler::submit`]; `input`/`not_found` for a run that does not
    /// exist; `input`/`schema_invalid` for a run that has ended or has a job
    /// already, a reconcile included, whose words then say when that
    /// reconcile is handed out.
    pub async fn enqueue(&self, enqueue: Enqueue) -> Result<JobId, Failure> {
        match self.resume(enqueue).await? {
            Resumed::Queued(job) => Ok(job),
            Resumed::Reconciling(found) => Err(failure::reconciling_refused(&found)),
        }
    }

    /// A person asks for the run now. In one transaction of the store:
    /// when the run's one job is a reconcile, the resume is not queued and
    /// is never lost. A reconcile that waits out its growing wait is woken
    /// to now; one that is leased is remembered, so that the reconcile it
    /// queues if it ends in doubt, or is lost, takes no growing wait. A
    /// quarantine that a lost lease began is kept either way, and
    /// [`Resumed::Reconciling`] says how long it still holds (decision
    /// 0193). Otherwise the resume's job is queued as
    /// [`Scheduler::enqueue`] queues it.
    ///
    /// # Errors
    ///
    /// As [`Scheduler::enqueue`], a reconcile aside.
    pub async fn resume(&self, enqueue: Enqueue) -> Result<Resumed, Failure> {
        let settings = self.inner.settings;
        let resumed = Arc::clone(&self.inner.resumed);
        let answer = self
            .inner
            .store
            .write(Durability::Synced, move |tx| {
                let answer = tables::resume(tx, &settings, enqueue)?;
                if let Resumed::Reconciling(found) = &answer
                    && found.leased
                {
                    resumed_set(&resumed).insert(found.job);
                }
                Ok(answer)
            })
            .await?;
        self.changed();
        Ok(answer)
    }

    /// Brings a reconcile of the run that waits out its growing wait
    /// forward to `at`: a named condition makes reading the system again
    /// worth it before then, such as a settle time the engine reported
    /// passing. Never before now, and never before the end of a quarantine
    /// a lost lease began. True when a reconcile was brought forward.
    ///
    /// # Errors
    ///
    /// `busy` failures of the store.
    pub async fn wake_reconcile(&self, run: RunId, at: Timestamp) -> Result<bool, Failure> {
        let settings = self.inner.settings;
        let woken = self
            .inner
            .store
            .write(Durability::Synced, move |tx| {
                tables::wake_reconcile(tx, &settings, run, at)
            })
            .await?;
        if woken {
            self.changed();
        }
        Ok(woken)
    }

    /// What follows runs cancelled in a transaction of another call, as
    /// [`Scheduler::cancel`] does after its own: `cancelled` when a run was
    /// cancelled at once, whose locks the ledger sorted; `freed` when a job
    /// let resources go.
    pub(crate) async fn after_cancels(&self, cancelled: bool, freed: bool) {
        if cancelled {
            self.inner.waits().locks_changed();
        }
        if freed {
            self.grant().await;
        }
        self.changed();
    }

    /// Cancels a run. A run no runner holds is cancelled at once. A run a
    /// runner holds is cancelled by that runner: it is told at its next
    /// heartbeat, finishes the commit point in hand, and ends the run.
    ///
    /// # Errors
    ///
    /// `input`/`not_found`; `busy` failures of the store.
    pub async fn cancel(&self, run: RunId) -> Result<CancelOutcome, Failure> {
        let (outcome, freed) = self
            .inner
            .store
            .write(Durability::Synced, move |tx| tables::cancel(tx, run))
            .await?;
        if outcome == CancelOutcome::Cancelled {
            // The run's locks were sorted by the ledger: some may be free.
            self.inner.waits().locks_changed();
        }
        if freed {
            self.grant().await;
        }
        self.changed();
        Ok(outcome)
    }

    // ---- runners ---------------------------------------------------------

    /// Records what a runner offers and tells it the terms: the heartbeat
    /// interval, the lease length, and the protocol version. A runner says
    /// this before it asks for work, and again whenever what it offers
    /// changes. The loop that runs work in the same process says it too.
    ///
    /// # Errors
    ///
    /// `unknown_variant`/`runner_protocol_unsupported`;
    /// `input`/`schema_invalid`.
    pub async fn hello(&self, offer: RunnerOffer) -> Result<RunnerTerms, Failure> {
        let settings = self.inner.settings;
        let terms = self
            .inner
            .store
            .write(Durability::Synced, move |tx| {
                tables::hello(tx, &settings, &offer)
            })
            .await?;
        self.changed();
        Ok(terms)
    }

    /// Hands the runner jobs it can serve, at most `free.total` and within
    /// its free slots for each mode. With nothing to hand out it waits, up
    /// to `wait` on the store's clock, for work to arrive; it never sleeps.
    /// Each lease carries a number the ledger checks from then on.
    ///
    /// A job is handed to one runner only: granting the lease, raising the
    /// run's lease number, and taking the job's exclusive resources are one
    /// transaction.
    ///
    /// # Errors
    ///
    /// `policy`/`runner_not_enrolled` for a runner that never said `hello`;
    /// `busy` failures of the store.
    pub async fn claim(
        &self,
        runner: RunnerId,
        free: Slots,
        wait: Duration,
    ) -> Result<Vec<JobLease>, Failure> {
        let settings = self.inner.settings;
        let deadline = plus(self.now(), wait);
        let mut timer = None;
        loop {
            // Listening starts before the look, so that work that arrives
            // between the look and the wait is not missed.
            let arrived = self.inner.work.notified();
            tokio::pin!(arrived);
            arrived.as_mut().enable();
            let leased = {
                let _gate = self.inner.gate.lock().await;
                let waited_for = self.inner.waits().waited_for();
                let free = free.clone();
                let leases = self.inner.leases.clone();
                self.inner
                    .store
                    .write(Durability::Synced, move |tx| {
                        tables::claim(tx, &settings, runner, free, &waited_for, leases.now())
                    })
                    .await?
            };
            if !leased.is_empty() {
                self.count(LEASED_COUNT, &[], leased.len());
                if let Some(logs_and_traces) = &self.inner.logs_and_traces {
                    for one in &leased {
                        logs_and_traces.observe(QUEUE_WAIT, &[], one.waited);
                    }
                }
                self.inner
                    .due
                    .send_modify(|count| *count = count.wrapping_add(1));
                return Ok(leased.into_iter().map(|one| one.lease).collect());
            }
            if self.now() >= deadline {
                return Ok(Vec::new());
            }
            let ended = timer.get_or_insert_with(|| self.timer(deadline));
            tokio::select! {
                () = &mut arrived => {}
                () = ended.fired() => return Ok(Vec::new()),
            }
        }
    }

    /// Renews a lease, and says how long it lasts now and what the runner
    /// should do: carry on, cancel, pause because the work is on hold, or
    /// drain.
    ///
    /// # Errors
    ///
    /// `policy`/`lease_superseded` when the lease is not the job's current
    /// one or ran out before the heartbeat arrived; `input`/`not_found`.
    pub async fn heartbeat(&self, lease: &LeaseRef) -> Result<Renewal, Failure> {
        let settings = self.inner.settings;
        let lease = *lease;
        let leases = self.inner.leases.clone();
        let renewal = self
            .inner
            .store
            .write(Durability::Relaxed, move |tx| {
                tables::heartbeat(tx, &settings, &lease, leases.now())
            })
            .await?;
        self.inner
            .due
            .send_modify(|count| *count = count.wrapping_add(1));
        Ok(renewal)
    }

    /// Records how a job ended, and frees what it held alone. A result is
    /// taken until the lease has been declared lost; after that it is
    /// refused, and the run is reconciled.
    ///
    /// With `Stopped` and a record in doubt, a reconcile job is queued, and
    /// what the job held alone passes to it. With `LeaseEnding` the job
    /// waits for a runner again.
    ///
    /// # Errors
    ///
    /// `policy`/`lease_superseded`; `input`/`not_found`.
    pub async fn finish(&self, lease: &LeaseRef, end: JobEnd) -> Result<(), Failure> {
        let settings = self.inner.settings;
        let lease = *lease;
        let resumed = Arc::clone(&self.inner.resumed);
        let ended = self
            .inner
            .store
            .write(Durability::Synced, move |tx| {
                // Read in the transaction, which the store's one writer runs
                // after every resume handed over before it.
                let asked = resumed_set(&resumed).contains(&lease.job);
                tables::finish(tx, &settings, &lease, &end, asked)
            })
            .await?;
        // The job has ended or gone back to waiting; no later resume names
        // it, so the mark is done with.
        resumed_set(&self.inner.resumed).remove(&lease.job);
        // The ledger sorted the run's locks when its state changed.
        self.inner.waits().locks_changed();
        if ended.freed {
            self.grant().await;
        }
        self.changed();
        Ok(())
    }

    /// Tells a runner to take no new work. The jobs it holds are told to
    /// drain at their next heartbeat.
    ///
    /// # Errors
    ///
    /// `policy`/`runner_not_enrolled`.
    pub async fn drain(&self, runner: RunnerId) -> Result<(), Failure> {
        self.inner
            .store
            .write(Durability::Synced, move |tx| {
                tables::set_runner_state(tx, runner, "draining")
            })
            .await
    }

    /// Records that a runner has left. A job it still holds is recovered
    /// when its lease runs out. A session whose home it was may move, if it
    /// is movable.
    ///
    /// # Errors
    ///
    /// `policy`/`runner_not_enrolled`.
    pub async fn bye(&self, runner: RunnerId) -> Result<(), Failure> {
        self.inner
            .store
            .write(Durability::Synced, move |tx| {
                tables::set_runner_state(tx, runner, "offline")
            })
            .await?;
        self.changed();
        Ok(())
    }

    // ---- exclusive resources, during a run -------------------------------

    /// Takes exclusive resources for a job during its run, all of them or
    /// none, and waits up to `wait` and never past the lease when one is
    /// held by another job. They are held until [`Scheduler::release`] or
    /// the end of the job.
    ///
    /// **The ordering rule.** Keys are ordered by their text. One call takes
    /// its keys together. A job that already holds keys, those it named in
    /// its requirements included, may ask only for keys that sort after
    /// every key it holds; a call that breaks the order is refused at once.
    /// So a job only ever waits for keys above the ones it holds, no circle
    /// of jobs waiting for each other can form, and there is no deadlock.
    /// Those who wait are served in the order they came.
    ///
    /// # Errors
    ///
    /// `busy`/`resource_order` for a call that breaks the order;
    /// `busy`/`resource_in_use` when the wait ran out;
    /// `policy`/`lease_superseded`.
    pub async fn acquire(
        &self,
        lease: &LeaseRef,
        keys: &[ResourceKey],
        wait: Duration,
    ) -> Result<(), Failure> {
        let mut keys = keys.to_vec();
        keys.sort();
        keys.dedup();
        let lease = *lease;
        let now = self.now();
        let (waiter, granted, deadline) = {
            let _gate = self.inner.gate.lock().await;
            let waited_for = self.inner.waits().waited_for();
            let wanted = keys.clone();
            let leases = self.inner.leases.clone();
            let acquired = self
                .inner
                .store
                .write(Durability::Synced, move |tx| {
                    tables::acquire(tx, &lease, &wanted, &waited_for, leases.now())
                })
                .await?;
            let lease_until = match acquired {
                Acquired::Taken => return Ok(()),
                Acquired::Blocked { lease_until } => self.inner.leases.to_store(lease_until),
            };
            if wait.is_zero() {
                return Err(failure::resource_in_use(self.inner.settings.lease_length));
            }
            let (waiter, granted) = self.inner.waits().add_resource_waiter(lease.job, keys);
            (waiter, granted, plus(now, wait).min(lease_until))
        };
        let mut timer = self.timer(deadline);
        tokio::select! {
            biased;
            answer = granted => match answer {
                Ok(()) => Ok(()),
                Err(_) => Err(failure::stopped()),
            },
            () = timer.fired() => {
                let _gate = self.inner.gate.lock().await;
                // Gone from the list means granted in the meantime.
                match self.inner.waits().remove_resource_waiter(waiter) {
                    Some(_) => Err(failure::resource_in_use(self.inner.settings.lease_length)),
                    None => Ok(()),
                }
            }
        }
    }

    /// Gives back resources the job took with [`Scheduler::acquire`], and
    /// hands them to those who wait.
    ///
    /// # Errors
    ///
    /// `policy`/`lease_superseded`.
    pub async fn release(&self, lease: &LeaseRef, keys: &[ResourceKey]) -> Result<(), Failure> {
        let lease = *lease;
        let keys = keys.to_vec();
        self.inner
            .store
            .write(Durability::Synced, move |tx| {
                tables::release(tx, &lease, &keys)
            })
            .await?;
        self.grant().await;
        self.changed();
        Ok(())
    }

    /// Hands freed resources to the jobs that wait for them during their
    /// runs, in order.
    async fn grant(&self) {
        let _gate = self.inner.gate.lock().await;
        let waiters = self.inner.waits().resource_waiters();
        if waiters.is_empty() {
            return;
        }
        let leases = self.inner.leases.clone();
        let granted = self
            .inner
            .store
            .write(Durability::Synced, move |tx| {
                tables::grant(tx, &waiters, leases.now())
            })
            .await;
        // When the store refuses, the waiters stay; their own deadlines end
        // their waits.
        if let Ok(granted) = granted {
            let mut waits = self.inner.waits();
            for id in granted {
                if let Some(waiter) = waits.remove_resource_waiter(id) {
                    // A waiter that stopped listening keeps the resources
                    // until its job ends; the job's end frees them.
                    let _ = waiter.granted.send(());
                }
            }
        }
    }

    // ---- record locks ----------------------------------------------------

    /// Notes the moment before a caller asks the ledger for a record lock.
    /// When the ledger answers that the lock is held, the caller waits with
    /// this ticket, and a release that happened in between is not missed.
    #[must_use]
    pub fn lock_ticket(&self, key: &ResourceKey) -> LockTicket {
        self.inner.waits().ticket(key)
    }

    /// The ledger refused a record lock as held: waits, behind those who
    /// came before, until the lock is freed, up to `wait` and never past the
    /// lease. `Ok` means "ask the ledger again", not "the lock is yours":
    /// the ledger alone grants a lock.
    ///
    /// The ledger records who holds a record lock, so the ordering rule of
    /// [`Scheduler::acquire`] does not reach these waits. What bounds them
    /// is the deadline: no wait outlasts `wait` or the lease.
    ///
    /// # Errors
    ///
    /// `busy`/`record_locked` with a retry time when the wait ran out;
    /// `policy`/`lease_superseded`.
    pub async fn wait_lock(
        &self,
        lease: &LeaseRef,
        ticket: LockTicket,
        wait: Duration,
    ) -> Result<(), Failure> {
        let lease = *lease;
        let leases = self.inner.leases.clone();
        let lease_until = self
            .inner
            .store
            .read(move |tx| tables::lease_until(tx, &lease, leases.now()))
            .await?;
        let deadline = plus(self.now(), wait).min(self.inner.leases.to_store(lease_until));
        let retry = self.inner.settings.lease_length;
        let (waiter, woken) = {
            let mut waits = self.inner.waits();
            if waits.freed_since(&ticket) {
                return Ok(());
            }
            if self.now() >= deadline {
                return Err(failure::record_locked(retry));
            }
            waits.add_lock_waiter(&ticket.key)
        };
        let mut timer = self.timer(deadline);
        tokio::select! {
            biased;
            _ = woken => Ok(()),
            () = timer.fired() => {
                if self.inner.waits().remove_lock_waiter(&ticket.key, waiter) {
                    Err(failure::record_locked(retry))
                } else {
                    Ok(())
                }
            }
        }
    }

    /// The ledger released a record lock: wakes the waiter that came first.
    /// Called by whoever saw the release commit.
    pub fn lock_freed(&self, key: &ResourceKey) {
        self.inner.waits().lock_freed(key);
    }

    // ---- budgets ---------------------------------------------------------

    /// Sets the limit of a rate budget. A new budget starts full; one that
    /// exists keeps what it holds, cut to the new capacity.
    ///
    /// # Errors
    ///
    /// `input`/`schema_invalid` for a limit that holds or refills nothing.
    pub async fn set_budget(&self, key: BudgetKey, limit: RateLimit) -> Result<(), Failure> {
        self.inner
            .store
            .write(Durability::Synced, move |tx| {
                budget::set_limit(tx, &key, &limit)
            })
            .await
    }

    /// Grants a job up to `want` requests against every one of `budgets` at
    /// once: its target's or surface's, its session's, its caller's. The
    /// grant is never larger than what the emptiest budget holds, and what
    /// is not spent is not returned. With nothing to grant, the call waits
    /// for the refill when that falls inside the lease.
    ///
    /// A budget refills by the store's clock and by nothing else, and what
    /// it holds is kept in the store, so a restart does not refill it.
    ///
    /// # Errors
    ///
    /// `busy`/`rate_limited` with the time to wait when the refill falls
    /// outside the lease; `policy`/`lease_superseded`;
    /// `input`/`schema_invalid` for no budget or no requests.
    pub async fn rate(
        &self,
        lease: &LeaseRef,
        budgets: &[BudgetKey],
        want: u32,
    ) -> Result<RateGrant, Failure> {
        if budgets.is_empty() {
            return Err(failure::invalid("a grant against no budget"));
        }
        let default = self.inner.settings.default_rate;
        let limits = self.inner.settings.watch;
        let lease = *lease;
        loop {
            let keys = budgets.to_vec();
            let leases = self.inner.leases.clone();
            let (take, lease_until) = self
                .inner
                .store
                .write(Durability::Relaxed, move |tx| {
                    let lease_until = tables::lease_until(tx, &lease, leases.now())?;
                    // A watch's check leaves its reserve of every budget to
                    // the caller's own reads (decision 0247), and spends the
                    // share its surface keeps for watches, however many
                    // requests it sends (decision 0262).
                    // A canary's check is held the same (decision 0999).
                    let run = tables::job_row(tx, lease.job)?.run;
                    let (check, canary) = match run {
                        Some(run) => (
                            tx.watches().check_of_run(run)?,
                            crate::canary::of_run(tx, run)?,
                        ),
                        None => (None, None),
                    };
                    let mut each: Vec<(BudgetKey, u32)> = keys
                        .into_iter()
                        .map(|key| {
                            let reserve = if check.is_some() || canary.is_some() {
                                limits.reserve_percent
                            } else {
                                0
                            };
                            (key, reserve)
                        })
                        .collect();
                    if let Some((watch, _)) = check {
                        let row = tx.watches().get(watch)?;
                        each.push((crate::watch::share_key(tx, &default, &limits, &row)?, 0));
                    } else if let Some(row) = canary {
                        each.push((crate::canary::share_key(tx, &default, &limits, &row)?, 0));
                    }
                    let take = budget::take_each(tx, &default, &each, want, false)?;
                    Ok((take, lease_until))
                })
                .await?;
            match take {
                Take::Granted { requests, over } => return Ok(RateGrant { requests, over }),
                Take::Wait { until } => {
                    if until >= self.inner.leases.to_store(lease_until) {
                        return Err(failure::rate_limited(between(self.now(), until)));
                    }
                    self.timer(until).fired().await;
                }
            }
        }
    }

    /// Grants all of `want` requests against every one of `budgets` at
    /// once, or nothing, without waiting, and never brings a budget below
    /// `reserve_percent` of its capacity: what a binding in shadow takes
    /// beside a caller's read (decision 0251), held to the same idea as a
    /// watch's check (decision 0247). `None` when it is not granted now.
    ///
    /// # Errors
    ///
    /// `policy`/`lease_superseded`; `input`/`schema_invalid` for no budget.
    pub async fn rate_now(
        &self,
        lease: &LeaseRef,
        budgets: &[BudgetKey],
        want: u32,
        reserve_percent: u32,
    ) -> Result<Option<RateGrant>, Failure> {
        if budgets.is_empty() {
            return Err(failure::invalid("a grant against no budget"));
        }
        let default = self.inner.settings.default_rate;
        let (lease, keys) = (*lease, budgets.to_vec());
        let leases = self.inner.leases.clone();
        let take = self
            .inner
            .store
            .write(Durability::Relaxed, move |tx| {
                tables::lease_until(tx, &lease, leases.now())?;
                budget::take(tx, &default, &keys, want.max(1), true, reserve_percent)
            })
            .await?;
        Ok(match take {
            Take::Granted { requests, over } => Some(RateGrant { requests, over }),
            Take::Wait { .. } => None,
        })
    }

    // ---- holds -----------------------------------------------------------

    /// Puts a scope on hold: no job it covers is handed out, a submission it
    /// covers is refused, and a runner that holds such a job is told to
    /// pause at its next heartbeat. `PauseScope::Everything` is the
    /// stop-everything switch. Whether a hold may be set or released is the
    /// policy engine's question, asked by the caller before this.
    ///
    /// # Errors
    ///
    /// `busy` failures of the store.
    pub async fn pause(&self, scope: PauseScope) -> Result<(), Failure> {
        self.inner
            .store
            .write(Durability::Synced, move |tx| tables::pause(tx, &scope))
            .await?;
        self.changed();
        Ok(())
    }

    /// Releases a hold. The jobs it covered are given back the time they
    /// were held, so the hold does not use up their queue limit. `false`
    /// when the scope was not on hold.
    ///
    /// # Errors
    ///
    /// `busy` failures of the store.
    pub async fn release_pause(&self, scope: PauseScope) -> Result<bool, Failure> {
        let released = self
            .inner
            .store
            .write(Durability::Synced, move |tx| {
                tables::release_pause(tx, &scope)
            })
            .await?;
        self.changed();
        Ok(released)
    }

    /// The scopes on hold.
    ///
    /// # Errors
    ///
    /// `busy` failures of the store.
    pub async fn pauses(&self) -> Result<Vec<PauseScope>, Failure> {
        self.inner
            .store
            .read(|tx| {
                Ok(tables::scope_pauses(tx)?
                    .into_iter()
                    .map(|hold| hold.scope)
                    .collect())
            })
            .await
    }

    // ---- time ------------------------------------------------------------

    /// Does what has come due on the store's clock: declares lost every
    /// lease that ended without a result, ends every job no runner took
    /// within its queue limit, and ends every wait whose time ran out.
    ///
    /// A lost lease is handled in one transaction (spec section 223): the
    /// job is marked lost; the run is given a new lease number that nobody
    /// holds, so the ledger refuses the lost runner from that moment and
    /// sorts the run's locks, quarantining those of records in doubt; the
    /// run stops as `possible_commit`, `runner_lost`; and a reconcile job
    /// is queued that no runner is given before the quarantine has passed.
    ///
    /// Called by one task, which waits on [`Scheduler::next_due`] between
    /// calls.
    ///
    /// # Errors
    ///
    /// `busy` failures of the store.
    pub async fn sweep(&self) -> Result<SweepReport, Failure> {
        let settings = self.inner.settings;
        let resumed = Arc::clone(&self.inner.resumed);
        let leases = self.inner.leases.clone();
        let swept = self
            .inner
            .store
            .write(Durability::Synced, move |tx| {
                let asked = resumed_set(&resumed).clone();
                tables::sweep(tx, &settings, &asked, leases.now())
            })
            .await?;
        {
            let mut resumed = resumed_set(&self.inner.resumed);
            for job in &swept.lost {
                resumed.remove(job);
            }
        }
        if !swept.lost.is_empty() || !swept.expired.is_empty() {
            self.inner.waits().locks_changed();
        }
        if swept.freed {
            self.grant().await;
        }
        let waits_ended = self.inner.waits().fire_timers(self.now());
        self.count(LOST_COUNT, &[], swept.lost.len());
        self.changed();
        Ok(SweepReport {
            lost: swept.lost,
            reconcile: swept.reconcile,
            expired: swept.expired,
            waits_ended,
        })
    }

    /// Declares lost at once every lease `runner` holds, because the runner
    /// says it holds none: the process that was this runner ended, and the
    /// runner starts again. Each lease is lost in one transaction, as
    /// [`Scheduler::sweep`] loses a lapsed one: the job is lost, the run is
    /// fenced by a new lease number and stops as `possible_commit`,
    /// `runner_lost`, and a reconcile is queued. Its quarantine is the job's
    /// longest step alone, the time a step the ended process sent can still
    /// be in flight, and not the setting, which is for a runner that may
    /// only be frozen (decision 0416).
    ///
    /// Call it only when no other process can be `runner`: a runner on one
    /// machine whose process holds the store's lock on its data folder, at
    /// its start and before it claims anything. A runner that is still
    /// working would be fenced in the middle of a step.
    ///
    /// # Errors
    ///
    /// `busy` failures of the store.
    pub async fn runner_ended(&self, runner: RunnerId) -> Result<SweepReport, Failure> {
        let settings = self.inner.settings;
        let resumed = Arc::clone(&self.inner.resumed);
        let swept = self
            .inner
            .store
            .write(Durability::Synced, move |tx| {
                let asked = resumed_set(&resumed).clone();
                tables::runner_ended(tx, &settings, &asked, runner)
            })
            .await?;
        {
            let mut resumed = resumed_set(&self.inner.resumed);
            for job in &swept.lost {
                resumed.remove(job);
            }
        }
        if !swept.lost.is_empty() {
            self.inner.waits().locks_changed();
        }
        if swept.freed {
            self.grant().await;
        }
        self.count(LOST_COUNT, &[], swept.lost.len());
        self.changed();
        Ok(SweepReport {
            lost: swept.lost,
            reconcile: swept.reconcile,
            expired: swept.expired,
            waits_ended: 0,
        })
    }

    /// The earliest moment at which [`Scheduler::sweep`] has something to
    /// do: a lease to declare lost, a queue limit, a job that becomes ready,
    /// a wait that ends. `None` when the scheduler waits for nothing.
    ///
    /// # Errors
    ///
    /// `busy` failures of the store.
    pub async fn next_due(&self) -> Result<Option<Timestamp>, Failure> {
        let settings = self.inner.settings;
        let leases = self.inner.leases.clone();
        let stored = self
            .inner
            .store
            .read(move |tx| tables::next_due(tx, &settings, &leases))
            .await?;
        let waited = self.inner.waits().next_timer();
        Ok(match (stored, waited) {
            (Some(stored), Some(waited)) => Some(stored.min(waited)),
            (one, other) => one.or(other),
        })
    }

    /// Tells the sweeping task when to read [`Scheduler::next_due`] again.
    #[must_use]
    pub fn due_watch(&self) -> DueWatch {
        DueWatch(self.inner.due.subscribe())
    }

    // ---- reading ---------------------------------------------------------

    /// The job of a run that is waiting or leased, when it has one, read in
    /// the caller's own transaction. A run has at most one. Whoever waits
    /// for work on a run to end reads this together with the run's row, so
    /// that a job a runner has claimed and not yet begun counts as work.
    ///
    /// # Errors
    ///
    /// The store's failures.
    pub fn unfinished_job(tx: &mut Tx<'_>, run: RunId) -> Result<Option<JobId>, Failure> {
        tables::unfinished_job(tx, run)
    }

    /// How many jobs are waiting or leased now, read in the caller's own
    /// transaction: the work the scheduler has in hand. Both counts are
    /// answered from an index, so this is cheap enough to ask often.
    ///
    /// # Errors
    ///
    /// The store's failures.
    pub fn unfinished_jobs(tx: &mut Tx<'_>) -> Result<u64, Failure> {
        tables::unfinished_jobs(tx)
    }

    /// A job, as the store holds it now.
    ///
    /// # Errors
    ///
    /// `input`/`not_found`.
    pub async fn job(&self, job: JobId) -> Result<JobRow, Failure> {
        self.inner
            .store
            .read(move |tx| tables::job_row(tx, job))
            .await
    }
}
