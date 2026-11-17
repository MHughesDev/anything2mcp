//! The scheduler's invariants, each against a real store in a folder of its
//! own. No test depends on real time: the clock moves only when a test moves
//! it, and a wait ends when a sweep finds that its moment has come.

#![allow(
    clippy::unwrap_used,
    reason = "this file holds only tests; clippy.toml allows unwrap in tests, which it tells by the #[test] attribute, and the helpers the tests share do not carry it"
)]

mod common;

use std::collections::BTreeSet;
use std::time::Duration;

use a2m_core::{Epoch, FailureClass, FailureCode, RunState, Timestamp};
use a2m_ledger::{Durability, Fence};
use a2m_sched::{
    Admitted, BudgetKey, CancelOutcome, Directive, Enqueue, HealthState, HoldScope, JobEnd,
    JobKind, JobState, Priority, RateLimit, ResourceKey, SchedSettings, SessionNeed, codes,
};
use common::*;

fn refused(result: Result<impl std::fmt::Debug, a2m_core::Failure>, code: &FailureCode) {
    let failure = result.unwrap_err();
    assert_eq!(failure.code(), code, "{failure}");
    assert!(!failure.next_step().is_empty());
}

// ---- admission -----------------------------------------------------------

#[tokio::test]
async fn a_run_is_admitted_with_its_job_and_the_same_call_gets_the_same_run() {
    let stand = Stand::new("admit").await;
    stand.hello(1).await;
    let (run, job) = stand.admit(read(1, "a")).await;
    // The store holds the run row and the job row, written together.
    assert_eq!(stand.run_state(run), "queued");
    assert_eq!(
        stand.stored(&format!("SELECT state FROM jobs WHERE id = '{job}'")),
        "pending"
    );
    // The same caller, tool version, and inputs: the same run (rule 6).
    assert_eq!(
        stand.sched.submit(read(1, "a")).await.unwrap(),
        Admitted::Existing { run }
    );
    assert_eq!(stand.counted("SELECT COUNT(*) FROM jobs"), 1);
    // Another caller with the same inputs gets a run of its own.
    let (other, _) = stand.admit(read(2, "a")).await;
    assert_ne!(other, run);
}

#[tokio::test]
async fn a_full_queue_and_a_caller_over_its_cap_are_refused_at_once_and_leave_nothing() {
    let settings = SchedSettings {
        queue_length: 3,
        caller_queue_cap: 2,
        ..SchedSettings::default()
    };
    let stand = Stand::with("bounds", settings).await;
    stand.hello(1).await;
    stand.admit(read(1, "a")).await;
    stand.admit(read(1, "b")).await;
    // The caller's cap: two jobs waiting.
    let over = stand.sched.submit(read(1, "c")).await;
    refused(over, &codes::CALLER_QUEUE_FULL);
    stand.admit(read(2, "a")).await;
    // The queue's length: three jobs waiting, whoever asks.
    let full = stand.sched.submit(read(3, "a")).await.unwrap_err();
    assert_eq!(full.class(), FailureClass::Busy);
    assert_eq!(full.code(), &codes::QUEUE_FULL);
    assert!(full.next_step().contains("Try again in about 30 seconds"));
    // A refusal is the whole transaction taken back: no run row, no job.
    assert_eq!(stand.counted("SELECT COUNT(*) FROM runs"), 3);
    assert_eq!(stand.counted("SELECT COUNT(*) FROM jobs"), 3);
}

#[tokio::test]
async fn health_missing_runners_and_spent_budgets_refuse_with_a_named_failure() {
    let stand = Stand::new("refusals").await;
    // No runner is enrolled yet.
    let none = stand.sched.submit(read(1, "a")).await.unwrap_err();
    assert_eq!(none.code(), &codes::RUNNER_UNAVAILABLE);
    assert!(none.message().contains("no runner is enrolled"));
    stand.hello(1).await;
    // A runner that offers another mode cannot serve the job.
    let mut other_mode = read(1, "a");
    other_mode.job.modes = vec![a2m_core::Mode::new("beta").unwrap()];
    let missing = stand.sched.submit(other_mode).await.unwrap_err();
    assert!(missing.message().contains("modes"), "{missing}");

    let cause = FailureCode::fixed("store_unavailable");
    let mut degraded = write(1, "w");
    degraded.health = HealthState::Degraded {
        cause: cause.clone(),
    };
    refused(stand.sched.submit(degraded).await, &codes::ENGINE_DEGRADED);
    // A degraded kernel still takes reads; a stopped one takes nothing.
    let mut reading = read(1, "r");
    reading.health = HealthState::Degraded {
        cause: cause.clone(),
    };
    stand.admit(reading).await;
    let mut stopped = read(1, "s");
    stopped.health = HealthState::Stopped { cause };
    refused(stand.sched.submit(stopped).await, &codes::ENGINE_DEGRADED);

    // A caller's budget of one submission a minute.
    let budget = BudgetKey::caller(workspace(), principal(5));
    let limit = RateLimit {
        capacity: 1,
        refill: 1,
        per: Duration::from_secs(60),
    };
    stand.sched.set_budget(budget.clone(), limit).await.unwrap();
    let mut first = read(5, "a");
    first.budgets = vec![budget.clone()];
    stand.admit(first).await;
    let mut second = read(5, "b");
    second.budgets = vec![budget];
    let limited = stand.sched.submit(second).await.unwrap_err();
    assert_eq!(limited.code(), &codes::RATE_LIMITED);
    assert!(limited.next_step().contains("60 seconds"), "{limited}");
    assert_eq!(stand.counted("SELECT COUNT(*) FROM jobs"), 2);
}

// ---- queues and claiming -------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_runners_cannot_hold_one_job() {
    let stand = Stand::new("race").await;
    for number in 1..=8 {
        stand.hello(number).await;
    }
    let mut jobs = BTreeSet::new();
    for input in ["a", "b", "c"] {
        jobs.insert(stand.admit(read(1, input)).await.1);
    }
    // Eight runners ask at the same moment, each for as many as there are.
    let claims: Vec<_> = (1..=8)
        .map(|number| {
            let sched = stand.sched.clone();
            tokio::spawn(async move { sched.claim(runner(number), slots(3), Duration::ZERO).await })
        })
        .collect();
    let mut handed = Vec::new();
    for claim in claims {
        handed.extend(claim.await.unwrap().unwrap());
    }
    // Every job was handed out, and each to one runner only.
    assert_eq!(handed.len(), 3);
    let distinct: BTreeSet<_> = handed.iter().map(|lease| lease.job).collect();
    assert_eq!(distinct, jobs);
    assert_eq!(
        stand.counted("SELECT COUNT(*) FROM jobs WHERE state = 'leased'"),
        3
    );
    assert!(stand.claim(runner(1), 3).await.is_empty());
}

#[tokio::test]
async fn interactive_goes_before_batch_and_callers_take_turns() {
    let stand = Stand::new("order").await;
    let one = stand.hello(1).await;
    // A batch job queued first, then a flood from one caller, then one job
    // from another caller.
    let mut nightly = read(3, "n");
    nightly.priority = Priority::Batch;
    let (_, batch) = stand.admit(nightly).await;
    let mut flood = Vec::new();
    for input in ["a", "b", "c", "d"] {
        flood.push(stand.admit(read(1, input)).await.1);
    }
    let (_, single) = stand.admit(read(2, "x")).await;

    let mut order = Vec::new();
    for _ in 0..6 {
        order.push(stand.claim_one(one).await.job);
    }
    // The other caller's one job is second, not sixth; batch is last.
    assert_eq!(
        order,
        vec![flood[0], single, flood[1], flood[2], flood[3], batch]
    );
}

#[tokio::test]
async fn a_claim_waits_for_work_and_ends_when_its_time_runs_out() {
    let stand = Stand::new("longpoll").await;
    let one = stand.hello(1).await;
    // Nothing is queued: the claim waits on the arrival of work.
    let sched = stand.sched.clone();
    let waiting =
        tokio::spawn(async move { sched.claim(one, slots(1), Duration::from_secs(30)).await });
    stand.waits_until(stand.after(30)).await;
    assert!(!waiting.is_finished());
    let (_, job) = stand.admit(read(1, "a")).await;
    let leases = waiting.await.unwrap().unwrap();
    assert_eq!(leases[0].job, job);

    // With no work at all, the wait ends when the clock says so.
    let sched = stand.sched.clone();
    let waiting =
        tokio::spawn(async move { sched.claim(one, slots(1), Duration::from_secs(30)).await });
    stand.waits_until(stand.after(30)).await;
    stand.advance(30);
    assert_eq!(stand.sched.sweep().await.unwrap().waits_ended, 1);
    assert!(waiting.await.unwrap().unwrap().is_empty());
}

#[tokio::test]
async fn a_lease_number_only_grows_and_an_older_one_is_refused() {
    let stand = Stand::new("epochs").await;
    let one = stand.hello(1).await;
    let (run, job) = stand.admit(read(1, "a")).await;
    let first = stand.claim_one(one).await;
    assert_eq!(
        (first.job, first.run, first.epoch),
        (job, Some(run), Epoch::new(1))
    );
    assert_eq!(first.length, Duration::from_secs(30));
    let renewal = stand.sched.heartbeat(&first.lease_ref()).await.unwrap();
    assert_eq!(renewal.directive, Directive::Continue);

    // The runner gives the job back; the next lease carries a higher number.
    stand
        .sched
        .finish(&first.lease_ref(), JobEnd::LeaseEnding)
        .await
        .unwrap();
    let second = stand.claim_one(one).await;
    assert_eq!(
        (second.job, second.epoch, second.attempt),
        (job, Epoch::new(2), 2)
    );

    // Everything that carries the older number is refused, here and in the
    // ledger.
    let old = first.lease_ref();
    refused(stand.sched.heartbeat(&old).await, &codes::LEASE_SUPERSEDED);
    refused(
        stand.sched.finish(&old, JobEnd::Done).await,
        &codes::LEASE_SUPERSEDED,
    );
    let fence = first.fence().unwrap();
    let stale = stand
        .store
        .write(Durability::Synced, move |tx| {
            tx.ledger().set_state(&fence, RunState::Validating, None)
        })
        .await
        .unwrap_err();
    assert_eq!(stale.code(), &codes::RUNNER_LOST);
    // A number that was never given is refused too.
    let mut forged = second.lease_ref();
    forged.epoch = Epoch::new(9);
    refused(
        stand.sched.heartbeat(&forged).await,
        &codes::LEASE_SUPERSEDED,
    );
    // The store itself refuses a lease number that goes down.
    let lowered = stand
        .raw()
        .execute(&format!("UPDATE jobs SET epoch = 1 WHERE id = '{job}'"), []);
    assert!(lowered.is_err());
}

// ---- lost leases ---------------------------------------------------------

#[tokio::test]
async fn a_lost_lease_is_fenced_and_quarantined_before_anyone_else_gets_the_run() {
    let stand = Stand::new("lost").await;
    let one = stand.hello(1).await;
    let two = stand.hello(2).await;
    let (run, job) = stand.admit(write(1, "w")).await;
    let lease = stand.claim_one(one).await;
    let fence = lease.fence().unwrap();
    // The runner stores an intent to write the record, and then freezes.
    start_writing(&stand.store, fence, "order-1", true).await;

    // Its lease runs out. Until the scheduler has declared it lost, nobody
    // else is given the job, and the runner's heartbeat is no longer taken.
    stand.advance(30);
    assert!(stand.sched.sweep().await.unwrap().lost.is_empty());
    assert!(stand.claim(two, 1).await.is_empty());
    refused(
        stand.sched.heartbeat(&lease.lease_ref()).await,
        &codes::LEASE_SUPERSEDED,
    );

    // Five seconds later it is lost: one transaction marks the job, fences
    // and stops the run, quarantines its lock, and queues the reconcile job.
    stand.advance(5);
    let report = stand.sched.sweep().await.unwrap();
    assert_eq!(report.lost, vec![job]);
    assert_eq!(report.reconcile.len(), 1);
    let reconcile = report.reconcile[0];
    assert_eq!(stand.sched.job(job).await.unwrap().state, JobState::Lost);
    assert_eq!(stand.run_state(run), "stopped");
    assert_eq!(
        stand.stored(&format!(
            "SELECT state_class || '/' || state_code FROM runs WHERE id = '{run}'"
        )),
        "possible_commit/runner_lost"
    );
    assert_eq!(
        stand.stored(&format!(
            "SELECT state FROM record_locks WHERE run_id = '{run}'"
        )),
        "quarantined"
    );

    // The lost runner thaws. The ledger refuses what it does, and the
    // scheduler refuses its result.
    let thawed = stand
        .store
        .write(Durability::Synced, move |tx| {
            tx.ledger().set_state(&fence, RunState::Running, None)
        })
        .await
        .unwrap_err();
    assert_eq!(thawed.code(), &codes::RUNNER_LOST);
    refused(
        stand.sched.finish(&lease.lease_ref(), JobEnd::Done).await,
        &codes::LEASE_SUPERSEDED,
    );

    // A second run that wants the same record cannot take its lock.
    stand.admit(write(2, "w")).await;
    let other = stand.claim_one(two).await;
    let other_fence = other.fence().unwrap();
    let locked = stand
        .store
        .write(Durability::Synced, move |tx| {
            let mut ledger = tx.ledger();
            ledger.set_state(&other_fence, RunState::Validating, None)?;
            ledger.plan_records(
                &other_fence,
                &[a2m_ledger::PlannedRecord {
                    step_seq: 0,
                    record_seq: 0,
                    key: key("order-1"),
                    operation: a2m_core::OperationName::new("example_order_update").unwrap(),
                    intended: intended("order-1"),
                    intended_values: None,
                    before: None,
                    unchanged: false,
                }],
            )?;
            ledger
                .take_lock(&other_fence, record(&other_fence))
                .map(|_| ())
        })
        .await
        .unwrap_err();
    assert_eq!(locked.code(), &codes::RECORD_LOCKED);

    // Nobody is given the reconcile job before the quarantine has passed.
    assert_eq!(
        stand.sched.job(reconcile).await.unwrap().available_at,
        stand.after(60)
    );
    stand
        .sched
        .finish(
            &other.lease_ref(),
            JobEnd::Stopped {
                class: FailureClass::Busy,
                code: codes::RECORD_LOCKED,
                in_doubt: false,
            },
        )
        .await
        .unwrap();
    stand.advance(59);
    assert!(stand.sched.sweep().await.unwrap().lost.is_empty());
    assert!(stand.claim(one, 1).await.is_empty());
    stand.advance(1);
    let again = stand.claim_one(one).await;
    assert_eq!(again.job, reconcile);
    assert_eq!(again.requirements.kind, JobKind::Reconcile);
    assert_eq!(again.run, Some(run));
    assert!(again.epoch > lease.epoch);
    // Under its new number the reconcile job confirms the quarantined lock.
    let new_fence = again.fence().unwrap();
    stand
        .store
        .write(Durability::Synced, move |tx| {
            tx.ledger()
                .take_lock(&new_fence, record(&new_fence))
                .map(|_| ())
        })
        .await
        .unwrap();
    assert_eq!(
        stand.stored(&format!(
            "SELECT state FROM record_locks WHERE run_id = '{run}'"
        )),
        "held"
    );
}

// ---- exclusive resources -------------------------------------------------

fn screen() -> ResourceKey {
    ResourceKey::new("screen", "example-desk").unwrap()
}

#[tokio::test]
async fn two_jobs_that_need_one_resource_run_one_after_the_other() {
    let settings = SchedSettings {
        queue_wait: Duration::from_secs(100),
        ..SchedSettings::default()
    };
    let stand = Stand::with("exclusive", settings).await;
    let one = stand.hello(1).await;
    let mut jobs = Vec::new();
    let mut runs = Vec::new();
    for input in ["a", "b", "c"] {
        let mut submission = read(1, input);
        submission.job.exclusive = vec![screen()];
        let (run, job) = stand.admit(submission).await;
        runs.push(run);
        jobs.push(job);
    }
    // A job that needs nothing alone is not held up by them.
    let (_, free) = stand.admit(read(2, "free")).await;

    // Three slots are free, and only one of the three is handed out.
    let leases = stand.claim(one, 4).await;
    let handed: Vec<_> = leases.iter().map(|lease| lease.job).collect();
    assert_eq!(handed, vec![jobs[0], free]);
    assert_eq!(
        stand
            .stored("SELECT job_id FROM resource_holds WHERE resource_key = 'screen:example-desk'"),
        jobs[0].to_string()
    );
    assert!(stand.claim(one, 4).await.is_empty());
    stand
        .sched
        .finish(&leases[1].lease_ref(), JobEnd::Done)
        .await
        .unwrap();

    // The first ends; the second runs.
    stand
        .sched
        .finish(&leases[0].lease_ref(), JobEnd::Done)
        .await
        .unwrap();
    let second = stand.claim_one(one).await;
    assert_eq!(second.job, jobs[1]);

    // The third waits past its queue limit: its deadline frees it, with the
    // reason, and its run stops as `busy`.
    stand.advance(20);
    stand.sched.heartbeat(&second.lease_ref()).await.unwrap();
    stand.advance(20);
    stand.sched.heartbeat(&second.lease_ref()).await.unwrap();
    stand.advance(20);
    stand.sched.heartbeat(&second.lease_ref()).await.unwrap();
    stand.advance(20);
    stand.sched.heartbeat(&second.lease_ref()).await.unwrap();
    stand.advance(20);
    let report = stand.sched.sweep().await.unwrap();
    assert_eq!(report.expired, vec![jobs[2]]);
    let third = stand.sched.job(jobs[2]).await.unwrap();
    assert_eq!(third.state, JobState::Dead);
    assert_eq!(
        third.end,
        Some((FailureClass::Busy, codes::RESOURCE_IN_USE))
    );
    assert_eq!(stand.run_state(runs[2]), "stopped");
    assert_eq!(
        stand.stored(&format!(
            "SELECT state_code FROM runs WHERE id = '{}'",
            runs[2]
        )),
        "resource_in_use"
    );
}

#[tokio::test]
async fn a_resource_taken_during_a_run_passes_to_the_waiter_and_a_deadline_ends_a_wait() {
    let stand = Stand::new("acquire").await;
    let one = stand.hello(1).await;
    stand.admit(read(1, "a")).await;
    stand.admit(read(2, "b")).await;
    stand.admit(read(3, "c")).await;
    let leases = stand.claim(one, 3).await;
    let (a, b, c) = (
        leases[0].lease_ref(),
        leases[1].lease_ref(),
        leases[2].lease_ref(),
    );
    let link = ResourceKey::new("link", "example-device").unwrap();
    let keys = [link.clone()];

    stand
        .sched
        .acquire(&a, &keys, Duration::ZERO)
        .await
        .unwrap();
    // Asking again for what the job holds changes nothing.
    stand
        .sched
        .acquire(&a, &keys, Duration::ZERO)
        .await
        .unwrap();
    // Without waiting, the second is told at once.
    refused(
        stand.sched.acquire(&b, &keys, Duration::ZERO).await,
        &codes::RESOURCE_IN_USE,
    );

    // B waits first, for twenty seconds; then C, for ten.
    let (sched, wanted) = (stand.sched.clone(), keys.clone());
    let b_waits =
        tokio::spawn(async move { sched.acquire(&b, &wanted, Duration::from_secs(20)).await });
    stand.waits_until(stand.after(20)).await;
    let (sched, wanted) = (stand.sched.clone(), keys.clone());
    let c_waits =
        tokio::spawn(async move { sched.acquire(&c, &wanted, Duration::from_secs(10)).await });
    stand.waits_until(stand.after(10)).await;

    // A releases. The one that came first is served; the other still waits.
    stand.sched.release(&a, &keys).await.unwrap();
    b_waits.await.unwrap().unwrap();
    assert_eq!(
        stand
            .stored("SELECT job_id FROM resource_holds WHERE resource_key = 'link:example-device'"),
        b.job.to_string()
    );
    assert!(!c_waits.is_finished());
    // C's deadline frees it, with the reason; the holder is not disturbed.
    stand.advance(10);
    assert_eq!(stand.sched.sweep().await.unwrap().waits_ended, 1);
    refused(c_waits.await.unwrap(), &codes::RESOURCE_IN_USE);
    assert_eq!(stand.counted("SELECT COUNT(*) FROM resource_holds"), 1);
    // The end of a job frees what it took.
    stand.sched.finish(&b, JobEnd::Done).await.unwrap();
    assert_eq!(stand.counted("SELECT COUNT(*) FROM resource_holds"), 0);
}

#[tokio::test]
async fn two_resources_taken_in_opposite_order_do_not_deadlock() {
    let stand = Stand::new("order-rule").await;
    let one = stand.hello(1).await;
    stand.admit(read(1, "a")).await;
    stand.admit(read(2, "b")).await;
    let leases = stand.claim(one, 2).await;
    let (a, b) = (leases[0].lease_ref(), leases[1].lease_ref());
    let first = ResourceKey::new("file", "example-1").unwrap();
    let second = ResourceKey::new("file", "example-2").unwrap();
    assert!(first < second);

    // A holds the first, B the second.
    stand
        .sched
        .acquire(&a, std::slice::from_ref(&first), Duration::ZERO)
        .await
        .unwrap();
    stand
        .sched
        .acquire(&b, std::slice::from_ref(&second), Duration::ZERO)
        .await
        .unwrap();
    // A asks for the second, in order, and waits for B.
    let (sched, wanted) = (stand.sched.clone(), second.clone());
    let a_waits =
        tokio::spawn(async move { sched.acquire(&a, &[wanted], Duration::from_secs(20)).await });
    stand.waits_until(stand.after(20)).await;
    // B asks for the first, against the order. It is refused at once and
    // does not wait: no circle forms.
    let against = stand
        .sched
        .acquire(&b, std::slice::from_ref(&first), Duration::from_secs(20))
        .await
        .unwrap_err();
    assert_eq!(against.class(), FailureClass::Busy);
    assert_eq!(against.code(), &codes::RESOURCE_ORDER);
    assert!(!a_waits.is_finished());
    // B does as it is told: it gives back the second, and A is served.
    stand
        .sched
        .release(&b, std::slice::from_ref(&second))
        .await
        .unwrap();
    a_waits.await.unwrap().unwrap();
    // Now B may take both together, behind A.
    stand.sched.finish(&a, JobEnd::Done).await.unwrap();
    stand
        .sched
        .acquire(&b, &[second, first], Duration::ZERO)
        .await
        .unwrap();
    assert_eq!(stand.counted("SELECT COUNT(*) FROM resource_holds"), 2);
}

#[tokio::test]
async fn waiters_for_a_record_lock_are_woken_in_order_and_none_waits_past_its_lease() {
    let stand = Stand::new("locks").await;
    let one = stand.hello(1).await;
    stand.admit(read(1, "a")).await;
    stand.admit(read(2, "b")).await;
    let leases = stand.claim(one, 2).await;
    let (a, b) = (leases[0].lease_ref(), leases[1].lease_ref());
    let lock = ResourceKey::record(workspace(), &target(), &environment(), &intended("order-1"));

    // A release between the look and the wait is not missed.
    let ticket = stand.sched.lock_ticket(&lock);
    stand.sched.lock_freed(&lock);
    stand
        .sched
        .wait_lock(&a, ticket, Duration::from_secs(600))
        .await
        .unwrap();

    // Two wait. B asks for ten minutes and is given until its lease ends,
    // thirty seconds from its claim; A, behind it, asks for ten seconds.
    let (sched, ticket) = (stand.sched.clone(), stand.sched.lock_ticket(&lock));
    let b_waits =
        tokio::spawn(async move { sched.wait_lock(&b, ticket, Duration::from_secs(600)).await });
    stand.waits_until(stand.after(30)).await;
    let (sched, ticket) = (stand.sched.clone(), stand.sched.lock_ticket(&lock));
    let a_waits =
        tokio::spawn(async move { sched.wait_lock(&a, ticket, Duration::from_secs(10)).await });
    stand.waits_until(stand.after(10)).await;

    // A release wakes the one that came first, and only that one.
    stand.sched.lock_freed(&lock);
    b_waits.await.unwrap().unwrap();
    assert!(!a_waits.is_finished());
    // A's own limit ends its wait, with a retry time.
    stand.advance(10);
    assert_eq!(stand.sched.sweep().await.unwrap().waits_ended, 1);
    let ran_out = a_waits.await.unwrap().unwrap_err();
    assert_eq!(ran_out.class(), FailureClass::Busy);
    assert_eq!(ran_out.code(), &codes::RECORD_LOCKED);
    assert!(ran_out.next_step().contains("Try again in about"));

    // B waits again, for ten minutes, and its lease ends in twenty seconds:
    // the wait ends with the lease.
    let (sched, ticket) = (stand.sched.clone(), stand.sched.lock_ticket(&lock));
    let b_waits =
        tokio::spawn(async move { sched.wait_lock(&b, ticket, Duration::from_secs(600)).await });
    stand.waits_until(stand.after(20)).await;
    stand.advance(20);
    assert_eq!(stand.sched.sweep().await.unwrap().waits_ended, 1);
    refused(b_waits.await.unwrap(), &codes::RECORD_LOCKED);
}

#[tokio::test]
async fn a_session_takes_as_many_jobs_at_once_as_its_limit_and_has_a_home() {
    let stand = Stand::new("session").await;
    let one = stand.hello(1).await;
    let two = stand.hello(2).await;
    let session = a2m_core::SessionId::from_bytes([7; 16]);
    let mut jobs = Vec::new();
    for input in ["a", "b"] {
        let mut submission = write(1, input);
        submission.job.session = Some(SessionNeed {
            session,
            home: Some(one),
            movable: true,
        });
        jobs.push(stand.admit(submission).await.1);
    }
    // The job goes to the session's home, not to another runner.
    assert!(stand.claim(two, 2).await.is_empty());
    // One write at a time on this session.
    let leases = stand.claim(one, 2).await;
    assert_eq!(leases.len(), 1);
    assert_eq!(leases[0].job, jobs[0]);
    stand
        .sched
        .finish(&leases[0].lease_ref(), JobEnd::Rests)
        .await
        .unwrap();
    // The home runner leaves; the session may move, so another takes it.
    stand.sched.bye(one).await.unwrap();
    assert_eq!(stand.claim_one(two).await.job, jobs[1]);
}

// ---- budgets -------------------------------------------------------------

#[tokio::test]
async fn budgets_refill_by_the_clock_only_and_a_restart_does_not_refill_them() {
    let stand = Stand::new("budgets").await;
    let one = stand.hello(1).await;
    stand.admit(read(1, "a")).await;
    let lease = stand.claim_one(one).await.lease_ref();
    let surface = BudgetKey::surface(workspace(), &target(), &environment(), "orders").unwrap();
    let caller = BudgetKey::caller(workspace(), principal(1));
    let per_second = RateLimit {
        capacity: 4,
        refill: 2,
        per: Duration::from_secs(1),
    };
    stand
        .sched
        .set_budget(surface.clone(), per_second)
        .await
        .unwrap();
    stand
        .sched
        .set_budget(
            caller.clone(),
            RateLimit {
                capacity: 100,
                refill: 100,
                per: Duration::from_secs(1),
            },
        )
        .await
        .unwrap();
    let budgets = [surface.clone(), caller];

    // A grant is never larger than what the emptiest budget holds.
    let grant = stand.sched.rate(&lease, &budgets, 20).await.unwrap();
    assert_eq!(grant.requests, 4);
    assert_eq!(grant.over, Duration::from_secs(2));

    // The bucket is empty. The next grant waits for the refill, and no
    // number of sweeps or calls gives it one while the clock stands still.
    let (sched, wanted) = (stand.sched.clone(), budgets.clone());
    let waiting = tokio::spawn(async move { sched.rate(&lease, &wanted, 3).await });
    stand
        .waits_until(Timestamp::from_unix_millis(
            stand.now().as_unix_millis() + 500,
        ))
        .await;
    for _ in 0..3 {
        assert_eq!(stand.sched.sweep().await.unwrap().waits_ended, 0);
        tokio::task::yield_now().await;
    }
    assert!(!waiting.is_finished());
    // One second of the clock is two requests.
    stand.advance(1);
    assert_eq!(stand.sched.sweep().await.unwrap().waits_ended, 1);
    assert_eq!(waiting.await.unwrap().unwrap().requests, 2);
    assert_eq!(
        stand.counted("SELECT level / per_ms FROM budgets WHERE budget_key LIKE 'surface:%'"),
        0
    );

    // A refill that falls outside the lease is a refusal with the time.
    let slow = BudgetKey::target(workspace(), &target(), &environment());
    stand
        .sched
        .set_budget(
            slow.clone(),
            RateLimit {
                capacity: 1,
                refill: 1,
                per: Duration::from_secs(3600),
            },
        )
        .await
        .unwrap();
    let slow = [slow];
    assert_eq!(
        stand.sched.rate(&lease, &slow, 1).await.unwrap().requests,
        1
    );
    refused(
        stand.sched.rate(&lease, &slow, 1).await,
        &codes::RATE_LIMITED,
    );

    // A restart keeps what the buckets held: still nothing to grant.
    let stand = stand.restart().await;
    refused(
        stand.sched.rate(&lease, &slow, 1).await,
        &codes::RATE_LIMITED,
    );
    assert_eq!(
        stand.counted("SELECT level / per_ms FROM budgets WHERE budget_key LIKE 'surface:%'"),
        0
    );
}

// ---- restart -------------------------------------------------------------

#[tokio::test]
async fn what_was_queued_or_leased_is_found_again_after_a_restart() {
    let stand = Stand::new("restart").await;
    let one = stand.hello(1).await;
    let (leased_run, leased_job) = stand.admit(read(1, "a")).await;
    let (_, queued_job) = stand.admit(read(1, "b")).await;
    let lease = stand.claim_one(one).await;
    assert_eq!(lease.job, leased_job);
    stand
        .sched
        .hold(HoldScope::Caller(workspace(), principal(9)))
        .await
        .unwrap();

    let stand = stand.restart().await;
    // The runner is known, the hold is kept, the queued job is handed out,
    // and the lease given before the restart is still the job's.
    assert_eq!(
        stand.sched.holds().await.unwrap(),
        vec![HoldScope::Caller(workspace(), principal(9))]
    );
    stand.sched.heartbeat(&lease.lease_ref()).await.unwrap();
    let after = stand.claim_one(one).await;
    assert_eq!(after.job, queued_job);
    stand
        .sched
        .finish(&after.lease_ref(), JobEnd::Done)
        .await
        .unwrap();

    // The runner that held the first job is gone: it never calls again. The
    // deadline rule recovers the job, and nothing was written, so no
    // reconcile job follows.
    assert_eq!(stand.sched.next_due().await.unwrap(), Some(stand.after(35)));
    stand.advance(35);
    let report = stand.sched.sweep().await.unwrap();
    assert_eq!(report.lost, vec![leased_job]);
    assert!(report.reconcile.is_empty());
    assert_eq!(stand.run_state(leased_run), "stopped");
    // Its caller resumes it with a new job, under a higher lease number.
    let resumed = stand
        .sched
        .enqueue(Enqueue {
            run: leased_run,
            job: requirements(false),
            priority: Priority::Interactive,
            health: HealthState::Ready,
            queue_wait: None,
        })
        .await
        .unwrap();
    let again = stand.claim_one(one).await;
    assert_eq!(again.job, resumed);
    assert!(again.epoch > lease.epoch);
}

// ---- cancel, holds, queue limits -----------------------------------------

#[tokio::test]
async fn a_run_is_cancelled_at_once_when_nobody_holds_it_and_through_the_heartbeat_otherwise() {
    let stand = Stand::new("cancel").await;
    let one = stand.hello(1).await;
    let (held_run, _) = stand.admit(read(1, "a")).await;
    let lease = stand.claim_one(one).await;
    let (waiting_run, waiting_job) = stand.admit(read(1, "b")).await;

    assert_eq!(
        stand.sched.cancel(waiting_run).await.unwrap(),
        CancelOutcome::Cancelled
    );
    assert_eq!(stand.run_state(waiting_run), "cancelled");
    assert_eq!(
        stand.sched.job(waiting_job).await.unwrap().state,
        JobState::Dead
    );
    assert!(stand.claim(one, 1).await.is_empty());
    assert_eq!(
        stand.sched.cancel(waiting_run).await.unwrap(),
        CancelOutcome::AlreadyFinal(RunState::Cancelled)
    );

    // The runner holds the other run: it is told, and ends the run itself.
    assert_eq!(
        stand.sched.cancel(held_run).await.unwrap(),
        CancelOutcome::Requested
    );
    assert_eq!(stand.run_state(held_run), "queued");
    let renewal = stand.sched.heartbeat(&lease.lease_ref()).await.unwrap();
    assert_eq!(renewal.directive, Directive::Cancel);
    let fence: Fence = lease.fence().unwrap();
    stand
        .store
        .write(Durability::Synced, move |tx| {
            tx.ledger().set_state(&fence, RunState::Cancelled, None)
        })
        .await
        .unwrap();
    stand
        .sched
        .finish(&lease.lease_ref(), JobEnd::Done)
        .await
        .unwrap();
    let ended = stand.sched.job(lease.job).await.unwrap();
    assert_eq!(
        (ended.state, ended.end_state),
        (JobState::Done, Some(RunState::Cancelled))
    );
}

#[tokio::test]
async fn the_stop_everything_switch_hands_out_nothing_and_pauses_what_runs() {
    let settings = SchedSettings {
        queue_wait: Duration::from_secs(60),
        ..SchedSettings::default()
    };
    let stand = Stand::with("hold", settings).await;
    let one = stand.hello(1).await;
    stand.admit(read(1, "a")).await;
    let lease = stand.claim_one(one).await;
    let (waiting_run, waiting_job) = stand.admit(read(1, "b")).await;

    stand.sched.hold(HoldScope::Everything).await.unwrap();
    refused(stand.sched.submit(read(2, "c")).await, &codes::ON_HOLD);
    assert!(stand.claim(one, 1).await.is_empty());
    let renewal = stand.sched.heartbeat(&lease.lease_ref()).await.unwrap();
    assert_eq!(renewal.directive, Directive::Pause);
    stand
        .sched
        .finish(
            &lease.lease_ref(),
            JobEnd::Stopped {
                class: FailureClass::Busy,
                code: codes::ON_HOLD,
                in_doubt: false,
            },
        )
        .await
        .unwrap();

    // The hold outlasts the waiting job's queue limit, and does not use it.
    stand.advance(90);
    assert!(stand.sched.sweep().await.unwrap().expired.is_empty());
    assert_eq!(stand.sched.next_due().await.unwrap(), None);
    assert!(
        stand
            .sched
            .release_hold(HoldScope::Everything)
            .await
            .unwrap()
    );
    assert!(stand.sched.sweep().await.unwrap().expired.is_empty());
    assert_eq!(stand.claim_one(one).await.job, waiting_job);
    assert_eq!(stand.run_state(waiting_run), "queued");

    // A hold on one caller leaves the others alone.
    stand
        .sched
        .hold(HoldScope::Caller(workspace(), principal(1)))
        .await
        .unwrap();
    refused(stand.sched.submit(read(1, "d")).await, &codes::ON_HOLD);
    stand.admit(read(2, "d")).await;
}

#[tokio::test]
async fn a_job_no_runner_takes_within_its_queue_limit_ends_its_run_as_busy() {
    let stand = Stand::new("queue-limit").await;
    let one = stand.hello(1).await;
    let mut submission = read(1, "a");
    submission.queue_wait = Some(Duration::from_secs(45));
    let (run, job) = stand.admit(submission).await;
    assert_eq!(stand.sched.next_due().await.unwrap(), Some(stand.after(45)));
    stand.sched.drain(one).await.unwrap();
    // A draining runner is given nothing.
    assert!(stand.claim(one, 1).await.is_empty());
    stand.advance(45);
    let report = stand.sched.sweep().await.unwrap();
    assert_eq!(report.expired, vec![job]);
    assert_eq!(stand.run_state(run), "stopped");
    assert_eq!(
        stand.stored(&format!(
            "SELECT state_class || '/' || state_code FROM runs WHERE id = '{run}'"
        )),
        "busy/runner_unavailable"
    );
    // A stopped run is not final: the same call is handed the same run, and
    // its caller resumes it.
    assert_eq!(
        stand.sched.submit(read(1, "a")).await.unwrap(),
        Admitted::Existing { run }
    );
    // A runner that never said hello is refused.
    refused(
        stand.sched.claim(runner(9), slots(1), Duration::ZERO).await,
        &codes::RUNNER_NOT_ENROLLED,
    );
}

// ---- the crate itself ----------------------------------------------------

#[test]
fn the_crate_never_sleeps_and_does_not_know_the_engine_or_a_pack() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
    for line in manifest
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
    {
        assert!(
            !line.contains("a2m-engine"),
            "the scheduler depends on the engine"
        );
        assert!(
            !line.contains("a2m-pack"),
            "the scheduler depends on a pack"
        );
    }
    for entry in std::fs::read_dir(root.join("src")).unwrap() {
        let path = entry.unwrap().path();
        let text = std::fs::read_to_string(&path).unwrap();
        for (number, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            assert!(
                !code.contains("sleep(")
                    && !code.contains("sleep_until(")
                    && !code.contains("interval("),
                "{}:{} waits on a fixed time",
                path.display(),
                number + 1
            );
            assert!(
                !code.contains("a2m_engine") && !code.contains("a2m_pack"),
                "{}:{} names the engine or a pack",
                path.display(),
                number + 1
            );
        }
    }
}
