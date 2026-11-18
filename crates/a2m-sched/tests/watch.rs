//! The scheduler's half of watches (decision 0247): due times from the
//! store's clock, a gap for each stretch of missed due times and one check
//! after it, a check behind interactive work and inside its share of the
//! budget, never two at once, never before the person confirmed it. The
//! store is asked what it holds; the clock moves only when told.

#![allow(
    clippy::unwrap_used,
    reason = "this file holds only tests; clippy.toml allows unwrap in tests, which it tells by the #[test] attribute, and the helpers the tests share do not carry it"
)]

mod common;

use std::collections::BTreeSet;
use std::time::Duration;

use a2m_core::{
    CanonicalJson, Digest, EffectClass, OperationName, RunState, SurfaceName, Timestamp, Values,
};
use a2m_ledger::{
    BlobKind, ChangeKind, Durability, NewBlob, NewRun, NewWatch, RetentionClass, WatchId,
    WatchReason, WatchRow, WatchShape, WatchState,
};
use a2m_sched::{
    BudgetKey, CheckAdmission, CheckStarted, DueCheck, HoldScope, JobKind, Priority, RateLimit,
    SHORTEST_INTERVAL_FLOOR, SchedSettings, Submission, WatchLimits, codes, due_at,
};
use common::*;

const INTERVAL: u64 = 300;

fn operation() -> OperationName {
    OperationName::new("example_order_list").unwrap()
}

fn new_watch(stand: &Stand, person: u8, interval: u64) -> NewWatch {
    NewWatch {
        workspace: workspace(),
        target: target(),
        environment: environment(),
        operation: operation(),
        target_version: Digest::of(b"plan"),
        effect: EffectClass::Read,
        input_hash: Digest::of(b"status=open"),
        input: None,
        ignore: BTreeSet::new(),
        surface: SurfaceName::new("orders").unwrap(),
        requests_per_check: 1,
        shape: WatchShape::Whole,
        interval: Duration::from_secs(interval),
        ends_at: stand.after(86_400),
        asked_by: principal(40),
        acts_for: principal(person),
        client_label: "example-agent".to_owned(),
        retention: RetentionClass::Long,
        condition: None,
    }
}

/// The check's run as the registry builds it from the watch's row.
fn check_of(watch: &WatchRow) -> Submission {
    let mut job = requirements(false);
    job.kind = JobKind::Canary;
    Submission {
        run: NewRun {
            principal: watch.acts_for,
            client_label: watch.id.to_string(),
            tool_name: watch.operation.clone(),
            input_hash: watch.input_hash,
            ..new_run(EffectClass::Read, 1, "unused")
        },
        job,
        // Whatever the caller says, a check is batch work.
        priority: Priority::Interactive,
        ..read(1, "unused")
    }
}

/// The published version the watches are asked for.
fn plan() -> Digest {
    Digest::of(b"plan")
}

/// The rate limit of the watches' surface, as the registry passes it at
/// confirmation: far more than any test's watches take.
fn roomy() -> RateLimit {
    RateLimit {
        capacity: 100,
        refill: 100,
        per: Duration::from_secs(1),
    }
}

async fn confirm(stand: &Stand, watch: WatchId, person: u8) -> WatchRow {
    stand
        .sched
        .confirm_watch(watch, principal(person), plan(), roomy())
        .await
        .unwrap()
}

async fn confirmed(stand: &Stand, person: u8) -> WatchRow {
    let asked = stand
        .sched
        .ask_watch(new_watch(stand, person, INTERVAL))
        .await
        .unwrap();
    confirm(stand, asked.id, person).await
}

fn first_due(watch: &WatchRow) -> Timestamp {
    due_at(
        watch.confirmed.unwrap().1,
        watch.phase.unwrap(),
        watch.interval,
        0,
    )
}

/// Moves the clock to `at`.
fn move_to(stand: &Stand, at: Timestamp) {
    let now = stand.now().as_unix_millis();
    let to = at.as_unix_millis();
    assert!(to >= now);
    stand
        .clock
        .advance(Duration::from_millis(u64::try_from(to - now).unwrap()));
}

async fn start(stand: &Stand, due: &DueCheck) -> CheckStarted {
    match stand
        .sched
        .submit_check(due, check_of(&due.watch))
        .await
        .unwrap()
    {
        CheckAdmission::Started(started) => started,
        CheckAdmission::Deferred { refused, .. } => panic!("deferred: {refused:?}"),
    }
}

/// The engine's part, done by hand: the check's run completes with a result
/// it stores, and its job ends; then the watch's check is ended with the
/// bytes of that result.
async fn complete(
    stand: &Stand,
    runner: a2m_core::RunnerId,
    watch: WatchId,
    started: &CheckStarted,
) {
    let lease = stand.claim_one(runner).await;
    assert_eq!(lease.run, Some(started.run));
    let fence = lease.fence().unwrap();
    let result = CanonicalJson::encode(&Values::new().with("count", 0_i64))
        .unwrap()
        .as_bytes()
        .to_vec();
    let stored = stand
        .store
        .blobs()
        .put(NewBlob {
            workspace: workspace(),
            owner: a2m_vault::OwnerRef::service("engine").unwrap(),
            kind: BlobKind::Result,
            class: RetentionClass::Long,
            plain: result.clone(),
        })
        .await
        .unwrap();
    stand
        .store
        .write(Durability::Synced, move |tx| {
            let mut ledger = tx.ledger();
            ledger.set_state(&fence, RunState::Validating, None)?;
            ledger.set_state(&fence, RunState::Running, None)?;
            ledger.store_result(&fence, stored)?;
            ledger.set_state(&fence, RunState::Completed, None)
        })
        .await
        .unwrap();
    stand
        .sched
        .finish(&lease.lease_ref(), a2m_sched::JobEnd::Done)
        .await
        .unwrap();
    let seq = started.check.seq;
    stand
        .store
        .write(Durability::Synced, move |tx| {
            tx.watches().end_check(watch, seq, Some(&result))
        })
        .await
        .unwrap();
}

async fn kinds(stand: &Stand, watch: WatchId) -> Vec<ChangeKind> {
    stand
        .store
        .read(move |tx| tx.watches().changes(watch, 0, 1000))
        .await
        .unwrap()
        .into_iter()
        .map(|change| change.kind)
        .collect()
}

#[test]
fn due_times_are_regular_spread_by_the_watch_and_never_random() {
    let limits = WatchLimits::default();
    let interval = Duration::from_secs(600);
    let mut phases = BTreeSet::new();
    for n in 0..50u8 {
        let watch = WatchId::from_bytes([n; 16]);
        let phase = limits.phase(watch, interval);
        assert_eq!(phase, limits.phase(watch, interval), "the same every time");
        assert!(
            phase < Duration::from_secs(60),
            "at most a tenth of the interval"
        );
        phases.insert(phase);
    }
    assert!(phases.len() > 40, "many watches do not fire together");
    // At most five minutes, however long the interval.
    let day = Duration::from_secs(86_400);
    for n in 0..20u8 {
        assert!(limits.phase(WatchId::from_bytes([n; 16]), day) < limits.jitter_most);
    }
    let start = Timestamp::from_unix_millis(START);
    let phase = Duration::from_millis(1234);
    for index in 0..5 {
        assert_eq!(
            due_at(start, phase, interval, index).as_unix_millis(),
            START + 1234 + i64::try_from(index).unwrap() * 600_000
        );
    }
}

#[test]
fn every_code_of_a_watch_the_scheduler_gives_is_in_its_table_once() {
    let codes = [
        (
            codes::WATCH_INTERVAL_TOO_SHORT,
            a2m_core::FailureClass::Input,
        ),
        (codes::WATCH_TOO_LONG, a2m_core::FailureClass::Input),
        (codes::WATCH_TOO_MANY, a2m_core::FailureClass::Busy),
        (codes::WATCH_OVER_BUDGET_SHARE, a2m_core::FailureClass::Busy),
    ];
    assert_eq!(codes.len(), codes::WATCH_CODES.len());
    for (code, class) in codes {
        assert!(codes::WATCH_CODES.contains(&(class, code.as_str())));
        assert!(
            !a2m_ledger::codes::WATCH_CODES
                .iter()
                .any(|(_, other)| *other == code.as_str()),
            "declared once"
        );
    }
}

#[test]
fn the_watches_on_a_surface_keep_to_their_share_of_its_budget() {
    let limits = WatchLimits::default();
    // Two requests a second: 172 800 a day, a tenth of it 17 280.
    let limit = RateLimit {
        capacity: 10,
        refill: 2,
        per: Duration::from_secs(1),
    };
    let minute = Duration::from_secs(60);
    // One request a minute is 1 440 a day: twelve such watches fit.
    let eleven = vec![(minute, 1); 11];
    limits.fits_budget(&limit, minute, 1, &eleven).unwrap();
    let twelve = vec![(minute, 1); 12];
    let over = limits.fits_budget(&limit, minute, 1, &twelve).unwrap_err();
    assert_eq!(over.code(), &codes::WATCH_OVER_BUDGET_SHARE);
}

#[tokio::test]
async fn a_watch_keeps_to_the_floor_the_longest_and_the_most_per_person() {
    // A setting below the floor does not lower it.
    let settings = SchedSettings {
        watch: WatchLimits {
            shortest_interval: Duration::from_secs(5),
            most_per_person: 2,
            ..WatchLimits::default()
        },
        ..SchedSettings::default()
    };
    let stand = Stand::with("watch-limits", settings).await;
    let fast = stand
        .sched
        .ask_watch(new_watch(&stand, 1, 30))
        .await
        .unwrap_err();
    assert_eq!(fast.code(), &codes::WATCH_INTERVAL_TOO_SHORT);
    let long = NewWatch {
        ends_at: stand.after(8 * 86_400),
        ..new_watch(&stand, 1, INTERVAL)
    };
    let refused = stand.sched.ask_watch(long).await.unwrap_err();
    assert_eq!(refused.code(), &codes::WATCH_TOO_LONG);
    stand
        .sched
        .ask_watch(new_watch(&stand, 1, SHORTEST_INTERVAL_FLOOR.as_secs()))
        .await
        .unwrap();
    stand
        .sched
        .ask_watch(new_watch(&stand, 1, INTERVAL))
        .await
        .unwrap();
    let many = stand
        .sched
        .ask_watch(new_watch(&stand, 1, INTERVAL))
        .await
        .unwrap_err();
    assert_eq!(many.code(), &codes::WATCH_TOO_MANY);
    // Another person has their own count.
    stand
        .sched
        .ask_watch(new_watch(&stand, 2, INTERVAL))
        .await
        .unwrap();
}

#[tokio::test]
async fn no_check_before_the_person_confirms_and_never_two_at_once() {
    let stand = Stand::new("watch-once").await;
    stand.hello(1).await;
    let asked = stand
        .sched
        .ask_watch(new_watch(&stand, 1, INTERVAL))
        .await
        .unwrap();
    stand.advance(INTERVAL * 3);
    assert_eq!(stand.sched.due_watches().await.unwrap(), Default::default());
    // Nothing is due of an asked watch but its end.
    assert_eq!(
        stand.sched.next_watch_due().await.unwrap(),
        Some(asked.ends_at)
    );
    assert_eq!(stand.counted("SELECT COUNT(*) FROM runs"), 0);

    let watch = confirm(&stand, asked.id, 1).await;
    let first = first_due(&watch);
    assert_eq!(stand.sched.next_watch_due().await.unwrap(), Some(first));
    assert!(stand.sched.due_watches().await.unwrap().checks.is_empty());
    move_to(&stand, first);
    let due = stand.sched.due_watches().await.unwrap().checks;
    assert_eq!(due.len(), 1);
    assert!(!due[0].due.catch_up);
    start(&stand, &due[0]).await;
    // While it runs, nothing else is due, and a second admission is refused.
    stand.advance(INTERVAL);
    assert!(stand.sched.due_watches().await.unwrap().checks.is_empty());
    let again = stand
        .sched
        .submit_check(&due[0], check_of(&due[0].watch))
        .await
        .unwrap_err();
    assert_eq!(again.code(), &a2m_ledger::codes::WATCH_CHECK_RUNNING);
    assert_eq!(stand.counted("SELECT COUNT(*) FROM runs"), 1);
}

#[tokio::test]
async fn a_check_waits_behind_interactive_work() {
    let stand = Stand::new("watch-behind").await;
    let runner = stand.hello(1).await;
    let watch = confirmed(&stand, 1).await;
    move_to(&stand, first_due(&watch));
    let due = stand.sched.due_watches().await.unwrap().checks;
    let check = start(&stand, &due[0]).await;
    // An interactive read queued after it goes first.
    let (read, _) = stand.admit(read(2, "a")).await;
    assert_eq!(stand.claim_one(runner).await.run, Some(read));
    assert_eq!(stand.claim_one(runner).await.run, Some(check.run));
    assert_eq!(
        stand.counted(&format!(
            "SELECT priority FROM jobs WHERE run_id = '{}'",
            check.run
        )),
        1,
        "batch"
    );
}

#[tokio::test]
async fn missed_due_times_are_one_gap_and_one_check_follows() {
    let stand = Stand::new("watch-gap").await;
    let runner = stand.hello(1).await;
    let watch = confirmed(&stand, 1).await;
    move_to(&stand, first_due(&watch));
    let due = stand.sched.due_watches().await.unwrap().checks;
    let started = start(&stand, &due[0]).await;
    complete(&stand, runner, watch.id, &started).await;

    // The engine stops; three due times pass; it starts again.
    stand.advance(INTERVAL * 3 + 1);
    let stand = stand.restart().await;
    let runner = stand.hello(1).await;
    let due = stand.sched.due_watches().await.unwrap().checks;
    assert_eq!(due.len(), 1);
    assert!(due[0].due.catch_up, "the one check after the gap");
    assert_eq!(due[0].due.index, 3);
    let changes = stand
        .store
        .read({
            let id = watch.id;
            move |tx| tx.watches().changes(id, 0, 100)
        })
        .await
        .unwrap();
    let gap = changes.last().unwrap();
    assert_eq!(gap.kind, ChangeKind::Gap);
    assert_eq!(gap.missed.unwrap().count, 2, "due times 1 and 2");
    let started = start(&stand, &due[0]).await;
    complete(&stand, runner, watch.id, &started).await;
    // Nothing is made up: two checks, two runs.
    assert_eq!(stand.counted("SELECT COUNT(*) FROM runs"), 2);
    assert_eq!(stand.counted("SELECT COUNT(*) FROM watch_checks"), 2);
    assert!(stand.sched.due_watches().await.unwrap().checks.is_empty());
}

#[tokio::test]
async fn a_hold_defers_the_check_and_leaves_a_gap_when_released() {
    let stand = Stand::new("watch-hold").await;
    stand.hello(1).await;
    let watch = confirmed(&stand, 1).await;
    stand
        .sched
        .hold(HoldScope::Target(workspace(), target()))
        .await
        .unwrap();
    move_to(&stand, first_due(&watch));
    let due = stand.sched.due_watches().await.unwrap().checks;
    let CheckAdmission::Deferred { refused, until } = stand
        .sched
        .submit_check(&due[0], check_of(&due[0].watch))
        .await
        .unwrap()
    else {
        panic!("a held target admits no check");
    };
    assert_eq!(refused.code(), &codes::ON_HOLD);
    assert_eq!(
        stand.counted("SELECT COUNT(*) FROM runs"),
        0,
        "nothing kept"
    );
    // Deferred: not due again before `until`.
    assert!(stand.sched.due_watches().await.unwrap().checks.is_empty());
    assert_eq!(stand.sched.next_watch_due().await.unwrap(), Some(until));
    stand.advance(INTERVAL * 2);
    stand
        .sched
        .release_hold(HoldScope::Target(workspace(), target()))
        .await
        .unwrap();
    let due = stand.sched.due_watches().await.unwrap().checks;
    assert!(due[0].due.catch_up);
    assert_eq!(kinds(&stand, watch.id).await.last(), Some(&ChangeKind::Gap));
    start(&stand, &due[0]).await;
}

/// Decision 0283 saw a check refused for its budget wait a whole interval,
/// past its next due time, and become a gap. It waits until the share
/// refills instead, which is well before its next due time here, though a
/// lease is longer than the interval.
#[tokio::test]
async fn a_check_refused_for_its_budget_waits_until_the_share_refills_and_misses_nothing() {
    let settings = SchedSettings {
        lease_length: Duration::from_secs(INTERVAL * 2),
        ..SchedSettings::default()
    };
    let stand = Stand::with("watch-refill", settings).await;
    stand.hello(1).await;
    // Ten requests every 100 seconds on the surface: the watches' tenth of
    // it holds one request and refills one every 100 seconds.
    let surface = BudgetKey::surface(workspace(), &target(), &environment(), "orders").unwrap();
    stand
        .sched
        .set_budget(
            surface,
            RateLimit {
                capacity: 10,
                refill: 10,
                per: Duration::from_secs(100),
            },
        )
        .await
        .unwrap();
    let first = confirmed(&stand, 1).await;
    // Another input, so that its check is a run of its own.
    let asked = stand
        .sched
        .ask_watch(NewWatch {
            input_hash: Digest::of(b"status=closed"),
            ..new_watch(&stand, 1, INTERVAL)
        })
        .await
        .unwrap();
    let second = confirm(&stand, asked.id, 1).await;
    move_to(&stand, first_due(&first).max(first_due(&second)));
    let due = stand.sched.due_watches().await.unwrap().checks;
    assert_eq!(due.len(), 2);
    start(&stand, &due[0]).await;
    let now = stand.now();
    let CheckAdmission::Deferred { refused, until } = stand
        .sched
        .submit_check(&due[1], check_of(&due[1].watch))
        .await
        .unwrap()
    else {
        panic!("the share holds one request, which the first check took");
    };
    assert_eq!(refused.code(), &codes::RATE_LIMITED);
    assert_eq!(refused.retry_after_ms(), Some(100_000));
    assert_eq!(
        until.as_unix_millis() - now.as_unix_millis(),
        100_000,
        "deferred until the share refills, not for a lease nor an interval"
    );
    let later = due[1].watch.id;
    let next_due = due_at(
        due[1].watch.confirmed.unwrap().1,
        due[1].watch.phase.unwrap(),
        due[1].watch.interval,
        due[1].due.index + 1,
    );
    assert!(until < next_due, "before its next due time");
    assert!(stand.sched.due_watches().await.unwrap().checks.is_empty());
    assert_eq!(stand.sched.next_watch_due().await.unwrap(), Some(until));
    // When the share has refilled, the same due time is checked: no gap.
    move_to(&stand, until);
    let due = stand.sched.due_watches().await.unwrap().checks;
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].watch.id, later);
    assert_eq!(due[0].due.index, 0);
    assert!(!due[0].due.catch_up);
    start(&stand, &due[0]).await;
    assert!(!kinds(&stand, later).await.contains(&ChangeKind::Gap));
}

#[tokio::test]
async fn a_check_leaves_its_share_of_each_budget_to_the_callers_own_reads() {
    let stand = Stand::new("watch-budget").await;
    let runner = stand.hello(1).await;
    // The watch's surface keeps a share for watches far larger than the
    // budget below, so that the reserve is what bounds the check here.
    let surface = BudgetKey::surface(workspace(), &target(), &environment(), "orders").unwrap();
    stand
        .sched
        .set_budget(
            surface,
            RateLimit {
                capacity: 1000,
                refill: 1000,
                per: Duration::from_secs(86_400),
            },
        )
        .await
        .unwrap();
    let budget = BudgetKey::new("surface", "example-orders").unwrap();
    // Ten requests, refilled by one a day: nothing refills in the test.
    let limit = RateLimit {
        capacity: 10,
        refill: 1,
        per: Duration::from_secs(86_400),
    };
    stand.sched.set_budget(budget.clone(), limit).await.unwrap();
    let watch = confirmed(&stand, 1).await;
    move_to(&stand, first_due(&watch));
    let due = stand.sched.due_watches().await.unwrap().checks;
    start(&stand, &due[0]).await;
    let check = stand.claim_one(runner).await;
    let grant = stand
        .sched
        .rate(&check.lease_ref(), std::slice::from_ref(&budget), 10)
        .await
        .unwrap();
    assert_eq!(grant.requests, 5, "half is left");
    let more = stand
        .sched
        .rate(&check.lease_ref(), std::slice::from_ref(&budget), 1)
        .await
        .unwrap_err();
    assert_eq!(more.code(), &codes::RATE_LIMITED);
    // The caller's own read takes what the check left.
    stand.admit(read(2, "own")).await;
    let own = stand.claim_one(runner).await;
    let grant = stand
        .sched
        .rate(&own.lease_ref(), std::slice::from_ref(&budget), 10)
        .await
        .unwrap();
    assert_eq!(grant.requests, 5);
}

#[tokio::test]
async fn a_watch_ends_at_its_end_and_ending_leaves_nothing_due() {
    let stand = Stand::new("watch-end").await;
    let asked = stand
        .sched
        .ask_watch(NewWatch {
            ends_at: stand.after(i64::try_from(INTERVAL * 2).unwrap()),
            ..new_watch(&stand, 1, INTERVAL)
        })
        .await
        .unwrap();
    let watch = confirm(&stand, asked.id, 1).await;
    stand.advance(INTERVAL * 3);
    assert!(stand.sched.due_watches().await.unwrap().checks.is_empty());
    let row = stand
        .store
        .read(move |tx| tx.watches().get(watch.id))
        .await
        .unwrap();
    assert_eq!(row.state, WatchState::Ended);
    assert_eq!(row.reason, Some(WatchReason::EndReached));
    let told = kinds(&stand, watch.id).await;
    assert_eq!(
        told,
        vec![ChangeKind::Confirmed, ChangeKind::Gap, ChangeKind::Ended]
    );
    assert_eq!(stand.sched.next_watch_due().await.unwrap(), None);
    assert_eq!(stand.counted("SELECT COUNT(*) FROM runs"), 0);
}

#[tokio::test]
async fn a_watch_past_its_end_ends_whatever_its_state() {
    let stand = Stand::new("watch-past-end").await;
    stand.hello(1).await;
    let short = |stand: &Stand| NewWatch {
        ends_at: stand.after(i64::try_from(INTERVAL * 2).unwrap()),
        ..new_watch(stand, 1, INTERVAL)
    };
    // One its person never confirmed, one paused, and one whose check is
    // under way when the end comes.
    let asked = stand.sched.ask_watch(short(&stand)).await.unwrap().id;
    let paused = stand.sched.ask_watch(short(&stand)).await.unwrap().id;
    confirm(&stand, paused, 1).await;
    stand
        .store
        .write(Durability::Synced, move |tx| {
            tx.watches().pause(paused, WatchReason::PausedByPerson)
        })
        .await
        .unwrap();
    let running = stand.sched.ask_watch(short(&stand)).await.unwrap().id;
    let row = confirm(&stand, running, 1).await;
    move_to(&stand, first_due(&row));
    let due = stand.sched.due_watches().await.unwrap().checks;
    assert_eq!(due.len(), 1);
    let started = start(&stand, &due[0]).await;

    stand.advance(INTERVAL * 2);
    let after = stand.sched.due_watches().await.unwrap();
    assert!(after.checks.is_empty());
    assert_eq!(
        after.cancelled,
        vec![started.run],
        "the check's run is cancelled with the watch's end"
    );
    assert_eq!(stand.run_state(started.run), "cancelled");
    for watch in [asked, paused, running] {
        let row = stand
            .store
            .read(move |tx| tx.watches().get(watch))
            .await
            .unwrap();
        assert_eq!(
            (row.state, row.reason),
            (WatchState::Ended, Some(WatchReason::EndReached)),
            "{watch}"
        );
    }
    let (open, not_ended) = stand
        .store
        .read(|tx| {
            let open = tx.watches().open()?;
            let not_ended = tx.watches().not_ended(&workspace(), &principal(1))?;
            Ok((open, not_ended))
        })
        .await
        .unwrap();
    assert!(open.is_empty());
    assert_eq!(not_ended, 0);
    assert_eq!(stand.sched.next_watch_due().await.unwrap(), None);
    assert_eq!(
        stand.stored(&format!(
            "SELECT outcome FROM watch_checks WHERE run_id = '{}'",
            started.run
        )),
        "failed"
    );
}

#[tokio::test]
async fn a_pass_that_fails_ends_nothing_and_cancels_nothing_and_the_next_does_both() {
    // The watch's end and its run's cancel are one transaction: a pass the
    // store refuses leaves the watch running on its run, so the next pass
    // still finds the run to cancel. Before, the runs to cancel were handed
    // out after the end had committed and a failure lost them.
    let stand = Stand::new("watch-end-and-cancel-together").await;
    stand.hello(1).await;
    let asked = stand
        .sched
        .ask_watch(NewWatch {
            ends_at: stand.after(i64::try_from(INTERVAL * 2).unwrap()),
            ..new_watch(&stand, 1, INTERVAL)
        })
        .await
        .unwrap();
    let row = confirm(&stand, asked.id, 1).await;
    move_to(&stand, first_due(&row));
    let due = stand.sched.due_watches().await.unwrap().checks;
    let started = start(&stand, &due[0]).await;
    assert!(started.job.is_some(), "the check opened its run");
    stand.advance(INTERVAL * 2);

    // The fault: the store refuses the run's cancel.
    stand
        .raw()
        .execute_batch(
            "CREATE TRIGGER fault_for_tests BEFORE UPDATE OF state ON runs
             WHEN NEW.state = 'cancelled'
             BEGIN SELECT RAISE(ABORT, 'a fault for the test'); END;",
        )
        .unwrap();
    assert!(stand.sched.due_watches().await.is_err());
    let id = asked.id;
    let watch = stand
        .store
        .read(move |tx| tx.watches().get(id))
        .await
        .unwrap();
    assert_eq!(watch.state, WatchState::Running, "the end did not commit");
    assert_ne!(stand.run_state(started.run), "cancelled");
    assert_eq!(
        stand.counted(&format!(
            "SELECT COUNT(*) FROM watch_checks WHERE run_id = '{}' AND outcome = 'running'",
            started.run
        )),
        1
    );

    stand
        .raw()
        .execute_batch("DROP TRIGGER fault_for_tests;")
        .unwrap();
    let after = stand.sched.due_watches().await.unwrap();
    assert_eq!(after.cancelled, vec![started.run]);
    assert_eq!(stand.run_state(started.run), "cancelled");
    let watch = stand
        .store
        .read(move |tx| tx.watches().get(id))
        .await
        .unwrap();
    assert_eq!(
        (watch.state, watch.reason),
        (WatchState::Ended, Some(WatchReason::EndReached))
    );
}

#[tokio::test]
async fn two_confirmations_racing_on_one_surface_admit_one() {
    let stand = Stand::new("watch-race").await;
    // One request every 30 seconds is 2 880 a day; a tenth of it, 288, is
    // one watch of one request every 300 seconds, and not two.
    let tight = RateLimit {
        capacity: 1,
        refill: 1,
        per: Duration::from_secs(30),
    };
    let one = stand
        .sched
        .ask_watch(new_watch(&stand, 1, INTERVAL))
        .await
        .unwrap()
        .id;
    let two = stand
        .sched
        .ask_watch(new_watch(&stand, 2, INTERVAL))
        .await
        .unwrap()
        .id;
    let (first, second) = tokio::join!(
        stand.sched.confirm_watch(one, principal(1), plan(), tight),
        stand.sched.confirm_watch(two, principal(2), plan(), tight),
    );
    let outcomes = [first, second];
    let admitted = outcomes.iter().filter(|outcome| outcome.is_ok()).count();
    assert_eq!(admitted, 1, "{outcomes:?}");
    let refused = outcomes
        .iter()
        .find_map(|outcome| outcome.as_ref().err())
        .unwrap();
    assert_eq!(refused.code(), &codes::WATCH_OVER_BUDGET_SHARE);
    assert_eq!(
        stand.counted("SELECT COUNT(*) FROM watches WHERE state = 'confirmed'"),
        1
    );
    // The refused one is still asked, and nothing of its confirmation stays.
    assert_eq!(
        stand
            .counted("SELECT COUNT(*) FROM watches WHERE state = 'asked' AND confirmed_by IS NULL"),
        1
    );
}

#[tokio::test]
async fn a_check_of_a_paged_read_waits_on_the_watch_share_of_the_surface() {
    let stand = Stand::new("watch-share").await;
    let runner = stand.hello(1).await;
    // A hundred requests a day; the watches on it share ten of them.
    let surface = BudgetKey::surface(workspace(), &target(), &environment(), "orders").unwrap();
    stand
        .sched
        .set_budget(
            surface.clone(),
            RateLimit {
                capacity: 100,
                refill: 100,
                per: Duration::from_secs(86_400),
            },
        )
        .await
        .unwrap();
    let watch = confirmed(&stand, 1).await;
    move_to(&stand, first_due(&watch));
    let due = stand.sched.due_watches().await.unwrap().checks;
    start(&stand, &due[0]).await;
    let check = stand.claim_one(runner).await;
    // A read of many pages asks for a hundred requests: it gets what is left
    // of the share, nine after the one its admission took, and then waits.
    let grant = stand
        .sched
        .rate(&check.lease_ref(), std::slice::from_ref(&surface), 100)
        .await
        .unwrap();
    assert_eq!(grant.requests, 9);
    let more = stand
        .sched
        .rate(&check.lease_ref(), std::slice::from_ref(&surface), 1)
        .await
        .unwrap_err();
    assert_eq!(more.code(), &codes::RATE_LIMITED);
    // The caller's own reads keep the rest of the surface.
    stand.admit(read(2, "own")).await;
    let own = stand.claim_one(runner).await;
    let grant = stand
        .sched
        .rate(&own.lease_ref(), std::slice::from_ref(&surface), 100)
        .await
        .unwrap();
    assert_eq!(grant.requests, 91);
}
