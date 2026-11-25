//! The scheduler's half of canaries on a schedule of their own (decision
//! 0999): a canary nobody confirmed is never due and sends nothing, and ends
//! at its end; only its own person confirms it, under the version it was
//! asked for; a confirmed canary's check is a read, admitted as batch work
//! of the kind `canary`; it goes on at its interval after a drift; and a
//! publish of another version ends it before another check. The store is
//! asked what it holds; the clock moves only when told.

#![allow(
    clippy::unwrap_used,
    reason = "this file holds only tests; clippy.toml allows unwrap in tests, which it tells by the #[test] attribute, and the helpers the tests share do not carry it"
)]

mod common;

use std::time::Duration;

use a2m_core::{Digest, EffectClass, FailureClass, FailureCode, OperationName, SurfaceName};
use a2m_ledger::NewRun;
use a2m_sched::{
    CanaryAdmission, CanaryEnd, CanaryOutcome, CanaryRow, CanaryState, DueCanary, JobKind,
    NewCanary, Priority, RateLimit, Submission, codes, due_at,
};
use common::*;

const INTERVAL: u64 = 300;

fn new_canary(stand: &Stand, person: u8) -> NewCanary {
    NewCanary {
        workspace: workspace(),
        target: target(),
        environment: environment(),
        operation: OperationName::new("example_order_list").unwrap(),
        target_version: Digest::of(b"plan"),
        effect: EffectClass::Read,
        input_hash: Digest::of(b"status=open"),
        input: None,
        surface: SurfaceName::new("orders").unwrap(),
        requests_per_check: 1,
        interval: Duration::from_secs(INTERVAL),
        ends_at: stand.after(3_600),
        asked_by: principal(40),
        acts_for: principal(person),
    }
}

/// The version `hash` of the target, published and made the one that runs,
/// as a publish leaves the store.
fn publish(stand: &Stand, hash: &Digest) {
    let raw = stand.raw();
    raw.execute(
        "UPDATE target_versions SET is_current = 0 WHERE workspace_id = ?1 AND target = ?2",
        rusqlite::params![workspace().to_string(), target().as_str()],
    )
    .unwrap();
    raw.execute(
        "INSERT INTO target_versions
            (workspace_id, target, plan_hash, version, plan, published_by, published_at, is_current)
         VALUES (?1, ?2, ?3, '1.0.0', x'00', ?4, 0, 1)",
        rusqlite::params![
            workspace().to_string(),
            target().as_str(),
            hash.to_string(),
            principal(1).to_string()
        ],
    )
    .unwrap();
}

fn roomy() -> RateLimit {
    RateLimit {
        capacity: 100,
        refill: 100,
        per: Duration::from_secs(1),
    }
}

/// The check's run as the registry builds it from the canary's row.
fn check_of(canary: &CanaryRow) -> Submission {
    let mut job = requirements(false);
    job.kind = JobKind::Canary;
    Submission {
        run: NewRun {
            principal: canary.acts_for,
            client_label: canary.id.to_string(),
            tool_name: canary.operation.clone(),
            input_hash: canary.input_hash,
            ..new_run(EffectClass::Read, 1, "unused")
        },
        job,
        // Whatever the caller says, a check is batch work.
        priority: Priority::Interactive,
        ..read(1, "unused")
    }
}

fn move_past(stand: &Stand, canary: &CanaryRow, index: u64) {
    let at = due_at(
        canary.confirmed.unwrap().1,
        canary.phase.unwrap(),
        canary.interval,
        index,
    );
    let now = stand.now().as_unix_millis();
    let to = at.as_unix_millis() + 1;
    if to > now {
        stand
            .clock
            .advance(Duration::from_millis(u64::try_from(to - now).unwrap()));
    }
}

async fn one_due(stand: &Stand) -> DueCanary {
    let mut due = stand.sched.due_canaries().await.unwrap();
    assert_eq!(due.len(), 1, "{due:?}");
    due.remove(0)
}

#[tokio::test]
async fn a_canary_nobody_confirmed_is_never_due_sends_nothing_and_ends_at_its_end() {
    let stand = Stand::new("canary-unconfirmed").await;
    publish(&stand, &Digest::of(b"plan"));
    let asked = stand.sched.ask_canary(new_canary(&stand, 7)).await.unwrap();
    assert_eq!(asked.state, CanaryState::Asked);
    // Many intervals pass: nothing is due, no job or run is made.
    for _ in 0..10 {
        stand.advance(INTERVAL);
        assert!(stand.sched.due_canaries().await.unwrap().is_empty());
    }
    assert_eq!(stand.counted("SELECT COUNT(*) FROM jobs"), 0);
    assert_eq!(stand.counted("SELECT COUNT(*) FROM runs"), 0);
    // Its end comes: it ends, still having sent nothing.
    stand.advance(INTERVAL * 3);
    assert!(stand.sched.due_canaries().await.unwrap().is_empty());
    let ended = stand.sched.canary(asked.id).await.unwrap();
    assert_eq!(ended.state, CanaryState::Ended);
    assert_eq!(ended.ended, Some(CanaryEnd::EndReached));
    assert_eq!(ended.checks, 0);
    assert_eq!(stand.counted("SELECT COUNT(*) FROM jobs"), 0);
    assert_eq!(stand.sched.next_canary_due().await.unwrap(), None);
    // An ended canary is never confirmed.
    let late = stand
        .sched
        .confirm_canary(asked.id, principal(7), Digest::of(b"plan"), roomy())
        .await
        .unwrap_err();
    assert_eq!(late.code(), &codes::CANARY_NOT_ASKED);
}

#[tokio::test]
async fn only_its_person_confirms_a_canary_and_only_under_the_version_it_was_asked_for() {
    let stand = Stand::new("canary-confirm").await;
    publish(&stand, &Digest::of(b"plan"));
    let asked = stand.sched.ask_canary(new_canary(&stand, 7)).await.unwrap();
    // Another person, even the caller that asked, is answered as if no such
    // canary were theirs.
    for other in [8, 40] {
        let refused = stand
            .sched
            .confirm_canary(asked.id, principal(other), Digest::of(b"plan"), roomy())
            .await
            .unwrap_err();
        assert_eq!(refused.class(), FailureClass::Input, "{refused:?}");
    }
    let refused = stand
        .sched
        .confirm_canary(asked.id, principal(7), Digest::of(b"other"), roomy())
        .await
        .unwrap_err();
    assert_eq!(refused.code(), &codes::CANARY_VERSION_CHANGED);
    // A write is no canary.
    let mut write = new_canary(&stand, 7);
    write.effect = EffectClass::Write;
    let refused = stand.sched.ask_canary(write).await.unwrap_err();
    assert_eq!(refused.code(), &codes::CANARY_NOT_A_READ);
    // Its person confirms it; its first due time is after the phase.
    let confirmed = stand
        .sched
        .confirm_canary(asked.id, principal(7), Digest::of(b"plan"), roomy())
        .await
        .unwrap();
    assert_eq!(confirmed.state, CanaryState::Confirmed);
    assert_eq!(confirmed.confirmed.unwrap().0, principal(7));
    assert!(confirmed.phase.unwrap() < Duration::from_secs(INTERVAL / 10));
    assert_eq!(
        stand.stored(&format!(
            "SELECT confirmed_by FROM canaries WHERE id = '{}'",
            asked.id
        )),
        principal(7).to_string()
    );
}

#[tokio::test]
async fn a_confirmed_canary_sends_a_batch_read_goes_on_after_a_drift_and_ends_on_a_new_version() {
    let stand = Stand::new("canary-checks").await;
    stand.hello(1).await;
    publish(&stand, &Digest::of(b"plan"));
    let asked = stand.sched.ask_canary(new_canary(&stand, 7)).await.unwrap();
    let canary = stand
        .sched
        .confirm_canary(asked.id, principal(7), Digest::of(b"plan"), roomy())
        .await
        .unwrap();
    move_past(&stand, &canary, 0);
    let due = one_due(&stand).await;
    assert_eq!(due.due.index, 0);

    // A check that would write is refused before anything is kept.
    let mut writes = check_of(&due.canary);
    writes.job.writes = true;
    let refused = stand.sched.submit_canary(&due, writes).await.unwrap_err();
    assert_eq!(refused.class(), FailureClass::Input);
    assert_eq!(stand.counted("SELECT COUNT(*) FROM jobs"), 0);

    let CanaryAdmission::Started { run, job } = stand
        .sched
        .submit_canary(&due, check_of(&due.canary))
        .await
        .unwrap()
    else {
        panic!("the check was deferred");
    };
    let job = job.unwrap();
    assert_eq!(
        stand.stored(&format!("SELECT kind FROM jobs WHERE id = '{job}'")),
        "canary"
    );
    assert_eq!(
        stand.counted(&format!("SELECT writes FROM jobs WHERE id = '{job}'")),
        0
    );
    assert_eq!(
        stand.counted(&format!("SELECT priority FROM jobs WHERE id = '{job}'")),
        1,
        "a check is batch work"
    );
    let running = stand.sched.canary(canary.id).await.unwrap();
    assert_eq!(running.state, CanaryState::Running);
    assert_eq!(running.running_run, Some(run));
    // One check at a time: nothing is due while it runs.
    move_past(&stand, &canary, 1);
    assert!(stand.sched.due_canaries().await.unwrap().is_empty());

    // The check found a drift: the canary goes on at its interval.
    let drift = FailureCode::new("screen_unrecognized").unwrap();
    let after = stand
        .sched
        .end_canary_check(
            canary.id,
            run,
            CanaryOutcome::Drifted,
            Some((FailureClass::Drift, drift.clone())),
        )
        .await
        .unwrap();
    assert_eq!(after.state, CanaryState::Confirmed);
    assert_eq!(after.checks, 1);
    assert_eq!(
        after
            .outcome
            .as_ref()
            .map(|last| (last.outcome, last.failure.clone())),
        Some((CanaryOutcome::Drifted, Some((FailureClass::Drift, drift))))
    );
    let due = one_due(&stand).await;
    assert_eq!(due.due.index, 1);

    // Another version is published: the canary ends before another check.
    publish(&stand, &Digest::of(b"repaired"));
    assert!(stand.sched.due_canaries().await.unwrap().is_empty());
    let ended = stand.sched.canary(canary.id).await.unwrap();
    assert_eq!(ended.ended, Some(CanaryEnd::VersionChanged));
    assert_eq!(stand.counted("SELECT COUNT(*) FROM jobs"), 1);
}
