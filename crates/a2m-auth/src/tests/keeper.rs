//! The keeper's invariants: who may have a lease, one renewer, the ladder
//! climbed once, `needs_login` when nothing can renew, and saved state that
//! survives a restart and opens for its owner alone.

use std::sync::Arc;
use std::time::Duration;

use a2m_core::{FailureClass, SessionState};
use a2m_vault::{ItemId, OwnerRef, SealContext, SealedKind};

use super::support::{
    HOUR, PERSON_ONLY, PERSON_RENEWING, STORED_HELD, STORED_RENEWING, Step, World, account_key,
    fake_entry, person_id, person_key, placed, request, settle,
};
use crate::{
    AuthEntry, EndedBy, ExpirySign, KeeperJob, KeeperSettings, PersonAnswer, PersonAsk,
    PersonReason, ProbeOutcome, ProviderName, RENEWALS, Returned, SessionRef, Wait, Work,
    WorkOutcome, codes,
};

#[tokio::test]
async fn a_lease_is_refused_for_another_persons_session() {
    let world = World::new(PERSON_RENEWING);
    let key = person_key(1);
    let entry = fake_entry();
    world.person.answers(|_| PersonAnswer::Done(Returned::new()));
    world
        .keeper
        .sign_in(&key, Arc::clone(&entry), Some(person_id(1)))
        .await
        .unwrap();
    settle().await;
    assert_eq!(world.state(&key).await, SessionState::Valid);

    // Another person, an agent acting for another person, and a caller that
    // acts for nobody: none of them gets the session, whatever its state.
    for other in [Some(person_id(2)), None] {
        let mut req = request(&key, &entry, Wait::NoWait);
        req.acting_for = other;
        let refused = world.keeper.lease(req).await.unwrap_err();
        assert_eq!(refused.class(), FailureClass::Policy);
        assert_eq!(refused.code(), &codes::IDENTITY_NOT_ALLOWED);
        let started = world
            .keeper
            .sign_in(&key, Arc::clone(&entry), other)
            .await
            .unwrap_err();
        assert_eq!(started.code(), &codes::IDENTITY_NOT_ALLOWED);
    }
    // Nothing was leased, and the person's own call is served.
    assert!(!world.kinds(&key).await.contains(&"leased"));
    let lease = world
        .keeper
        .lease(request(&key, &entry, Wait::NoWait))
        .await
        .unwrap();
    assert_eq!(lease.state(), SessionState::Valid);
    assert!(world.kinds(&key).await.contains(&"leased"));
}

#[tokio::test]
async fn ten_runs_meet_an_expired_session_and_exactly_one_renewal_happens() {
    let world = World::new(STORED_RENEWING);
    let key = account_key("robot");
    let entry = fake_entry();
    let first = world
        .keeper
        .lease(request(&key, &entry, Wait::DEFAULT))
        .await
        .unwrap();
    assert_eq!((first.epoch(), world.fake.sign_ins()), (1, 1));
    assert_eq!(placed(&first)[0].1, "PLANTED-fake-1-0");

    // The credentials' own expiry passes. Ten runs ask at once, and the
    // renewal is held so that all ten are waiting on it.
    world.clock.advance(2 * HOUR);
    world.fake.hold_renewals();
    let runs: Vec<_> = (0..10)
        .map(|_| {
            let keeper = world.keeper.clone();
            let req = request(&key, &entry, Wait::DEFAULT);
            tokio::spawn(async move { keeper.lease(req).await })
        })
        .collect();
    settle().await;
    assert_eq!(world.fake.refreshes(), 1);
    assert_eq!(world.state(&key).await, SessionState::Renewing);

    world.fake.release_renewals();
    for run in runs {
        let lease = run.await.unwrap().unwrap();
        assert_eq!(lease.epoch(), 2);
        assert_eq!(placed(&lease)[0].1, "PLANTED-fake-1-1");
    }
    assert_eq!((world.fake.refreshes(), world.fake.sign_ins()), (1, 1));
    let kinds = world.kinds(&key).await;
    assert_eq!(kinds.iter().filter(|kind| **kind == "renewed").count(), 1);
    assert_eq!(
        kinds.iter().filter(|kind| **kind == "renewal_started").count(),
        1
    );
    assert_eq!(world.telemetry.metrics().counter(RENEWALS, &[]), 1);

    // The lease from before the renewal is stale, and says so.
    let mut sink = super::support::Sink::default();
    let stale = first.apply(&mut sink).unwrap_err();
    assert_eq!(stale.code(), &codes::LEASE_STALE);
    assert!(sink.placed.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_renewal_also_under_real_parallelism() {
    let world = World::new(STORED_RENEWING);
    let key = account_key("robot");
    let entry = fake_entry();
    world
        .keeper
        .lease(request(&key, &entry, Wait::DEFAULT))
        .await
        .unwrap();
    world.clock.advance(2 * HOUR);
    let runs: Vec<_> = (0..10)
        .map(|_| {
            let keeper = world.keeper.clone();
            let req = request(&key, &entry, Wait::DEFAULT);
            tokio::spawn(async move { keeper.lease(req).await })
        })
        .collect();
    for run in runs {
        assert_eq!(run.await.unwrap().unwrap().epoch(), 2);
    }
    assert_eq!((world.fake.refreshes(), world.fake.sign_ins()), (1, 1));
}

#[tokio::test]
async fn ten_reports_of_one_sign_cause_one_check_and_a_stale_report_changes_nothing() {
    let world = World::new(STORED_RENEWING);
    let key = account_key("robot");
    let entry = fake_entry();
    let mut leases = Vec::new();
    for _ in 0..10 {
        leases.push(
            world
                .keeper
                .lease(request(&key, &entry, Wait::DEFAULT))
                .await
                .unwrap(),
        );
    }
    // The driver asks the lease what it saw, and only reports.
    let sign = leases[0]
        .detect_expiry(&crate::RawSign::Status {
            code: 401,
            challenge: None,
        })
        .unwrap();
    assert_eq!(sign, ExpirySign::Refused);
    assert!(
        leases[0]
            .detect_expiry(&crate::RawSign::Status {
                code: 500,
                challenge: None
            })
            .is_none()
    );

    // The probe says the session is gone, so the one re-check starts the
    // one renewal.
    *world.probe.outcome.lock().unwrap() = ProbeOutcome::SignedOut;
    for lease in &leases {
        world.keeper.report(lease, sign).await;
    }
    settle().await;
    assert_eq!(world.probe.calls(), 1);
    assert_eq!(world.fake.refreshes(), 1);
    assert_eq!(world.state(&key).await, SessionState::Valid);

    // The same leases report again: their epoch is no longer the session's.
    for lease in &leases {
        world.keeper.report(lease, sign).await;
    }
    settle().await;
    assert_eq!((world.probe.calls(), world.fake.refreshes()), (1, 1));

    // A sign on the new credentials that the probe does not confirm is one
    // check and no renewal.
    *world.probe.outcome.lock().unwrap() = ProbeOutcome::SignedIn;
    let fresh = world
        .keeper
        .lease(request(&key, &entry, Wait::DEFAULT))
        .await
        .unwrap();
    world.keeper.report(&fresh, sign).await;
    settle().await;
    assert_eq!((world.probe.calls(), world.fake.refreshes()), (2, 1));
    assert_eq!(world.state(&key).await, SessionState::Valid);
}

#[tokio::test]
async fn an_expired_session_with_no_renewal_path_gives_needs_login_to_every_call() {
    let world = World::new(PERSON_ONLY);
    let key = person_key(1);
    let entry = fake_entry();

    // Never signed in: `needs_login`, and nothing was tried.
    let absent = world
        .keeper
        .lease(request(&key, &entry, Wait::NoWait))
        .await
        .unwrap_err();
    assert_eq!(absent.class(), FailureClass::SessionExpired);
    assert_eq!(absent.code(), &codes::NEEDS_LOGIN);
    assert_eq!(world.fake.sign_ins(), 0);

    world
        .keeper
        .sign_in(&key, Arc::clone(&entry), Some(person_id(1)))
        .await
        .unwrap();
    settle().await;
    world
        .keeper
        .lease(request(&key, &entry, Wait::NoWait))
        .await
        .unwrap();

    // Its credentials run out. There is no silent rung, so the person is
    // told once and every call stops.
    world.clock.advance(2 * HOUR);
    for _ in 0..5 {
        let stopped = world
            .keeper
            .lease(request(&key, &entry, Wait::NoWait))
            .await
            .unwrap_err();
        assert_eq!(stopped.code(), &codes::NEEDS_LOGIN);
    }
    assert_eq!(world.state(&key).await, SessionState::NeedsPerson);
    assert_eq!(world.person.notices(), 1);
    assert_eq!((world.fake.sign_ins(), world.fake.refreshes()), (1, 0));

    // An interactive call waits for the person, and stops when its wait is
    // over. The clock is the test's; the tick is what looks at it.
    let waiting = {
        let keeper = world.keeper.clone();
        let req = request(&key, &entry, Wait::Interactive(Duration::from_secs(300)));
        tokio::spawn(async move { keeper.lease(req).await })
    };
    settle().await;
    assert!(!waiting.is_finished());
    world.clock.advance(Duration::from_secs(299));
    world.keeper.tick().await;
    settle().await;
    assert!(!waiting.is_finished());
    world.clock.advance(Duration::from_secs(1));
    world.keeper.tick().await;
    assert_eq!(
        waiting.await.unwrap().unwrap_err().code(),
        &codes::NEEDS_LOGIN
    );

    // The person signs in again, and calls run.
    world
        .keeper
        .sign_in(&key, Arc::clone(&entry), Some(person_id(1)))
        .await
        .unwrap();
    settle().await;
    let lease = world
        .keeper
        .lease(request(&key, &entry, Wait::NoWait))
        .await
        .unwrap();
    assert_eq!(lease.epoch(), 2);

    // Ended on purpose: `needs_login`, the old lease places nothing, and
    // nothing is retried.
    world.keeper.revoke(&key, EndedBy::Owner).await.unwrap();
    let revoked = world
        .keeper
        .lease(request(&key, &entry, Wait::NoWait))
        .await
        .unwrap_err();
    assert_eq!(revoked.code(), &codes::NEEDS_LOGIN);
    let mut sink = super::support::Sink::default();
    assert_eq!(
        lease.apply(&mut sink).unwrap_err().code(),
        &codes::LEASE_STALE
    );
    assert!(world.store.saved(lease.session()).await.unwrap().is_none());
    assert_eq!(world.fake.sign_ins(), 2);
}

#[tokio::test]
async fn a_second_factor_nobody_gives_is_needs_mfa() {
    let world = World::new(PERSON_RENEWING);
    let key = person_key(1);
    let entry = fake_entry();
    world
        .keeper
        .sign_in(&key, Arc::clone(&entry), Some(person_id(1)))
        .await
        .unwrap();
    settle().await;
    world.clock.advance(2 * HOUR);
    *world.fake.on_refresh.lock().unwrap() = Step::WantsSecondFactor;
    let stopped = world
        .keeper
        .lease(request(&key, &entry, Wait::NoWait))
        .await
        .unwrap_err();
    assert_eq!(stopped.code(), &codes::NEEDS_MFA);
    let status = world
        .keeper
        .status(&key.workspace, Some(&key.identity))
        .await
        .unwrap();
    assert_eq!(status[0].state, SessionState::NeedsPerson);
    assert_eq!(status[0].wants, Some(PersonReason::SecondFactor));
    assert!(matches!(
        world.person.asks.lock().unwrap()[0],
        PersonAsk::Notify {
            reason: PersonReason::SecondFactor
        }
    ));
}

#[tokio::test]
async fn a_refused_refresh_goes_to_the_person_and_is_never_tried_again() {
    let world = World::new(PERSON_RENEWING);
    let key = person_key(1);
    let entry = fake_entry();
    world
        .keeper
        .sign_in(&key, Arc::clone(&entry), Some(person_id(1)))
        .await
        .unwrap();
    settle().await;
    world.clock.advance(2 * HOUR);
    *world.fake.on_refresh.lock().unwrap() = Step::Refused;
    for _ in 0..10 {
        let stopped = world
            .keeper
            .lease(request(&key, &entry, Wait::NoWait))
            .await
            .unwrap_err();
        assert_eq!(stopped.code(), &codes::NEEDS_LOGIN);
        world.keeper.tick().await;
    }
    assert_eq!(world.state(&key).await, SessionState::NeedsPerson);
    assert_eq!((world.fake.refreshes(), world.fake.sign_ins()), (1, 1));
    assert_eq!(world.person.notices(), 1);
}

#[tokio::test]
async fn a_refused_stored_credential_is_not_tried_again() {
    // At sign-in.
    let world = World::new(STORED_RENEWING);
    let key = account_key("robot");
    let entry = fake_entry();
    *world.fake.on_sign_in.lock().unwrap() = Step::Refused;
    for _ in 0..10 {
        let stopped = world
            .keeper
            .lease(request(&key, &entry, Wait::DEFAULT))
            .await
            .unwrap_err();
        assert_eq!(stopped.class(), FailureClass::SessionExpired);
        assert_eq!(stopped.code(), &codes::CREDENTIAL_REFUSED);
    }
    assert_eq!(world.fake.sign_ins(), 1);
    assert_eq!(world.state(&key).await, SessionState::NeedsPerson);
    assert_eq!(world.person.notices(), 1);

    // A person looks at it and asks for a sign-in: one more try, no more.
    world.keeper.sign_in(&key, Arc::clone(&entry), None).await.unwrap();
    settle().await;
    assert_eq!(world.fake.sign_ins(), 2);
    assert_eq!(world.state(&key).await, SessionState::Absent);
    *world.fake.on_sign_in.lock().unwrap() = Step::Works;
    world.keeper.sign_in(&key, Arc::clone(&entry), None).await.unwrap();
    settle().await;
    assert_eq!(world.state(&key).await, SessionState::Valid);

    // At renewal: the same credential, refused, is counted once and not
    // offered again on a higher rung.
    world.clock.advance(2 * HOUR);
    *world.fake.on_refresh.lock().unwrap() = Step::Refused;
    *world.fake.on_sign_in.lock().unwrap() = Step::Refused;
    for _ in 0..10 {
        let stopped = world
            .keeper
            .lease(request(&key, &entry, Wait::DEFAULT))
            .await
            .unwrap_err();
        assert_eq!(stopped.code(), &codes::CREDENTIAL_REFUSED);
    }
    assert_eq!((world.fake.refreshes(), world.fake.sign_ins()), (1, 3));
    assert_eq!(world.state(&key).await, SessionState::NeedsPerson);
}

#[tokio::test]
async fn a_held_credential_the_system_refuses_is_offered_once_and_no_more() {
    let world = World::new(STORED_HELD);
    let key = account_key("robot");
    let entry = fake_entry();
    let lease = world
        .keeper
        .lease(request(&key, &entry, Wait::DEFAULT))
        .await
        .unwrap();
    // The system refuses it, and the probe agrees. The credential was never
    // confirmed, and the limit is one attempt: it is not offered again.
    *world.probe.outcome.lock().unwrap() = ProbeOutcome::SignedOut;
    world.keeper.report(&lease, ExpirySign::Refused).await;
    settle().await;
    assert_eq!(world.state(&key).await, SessionState::NeedsPerson);
    assert_eq!(world.fake.sign_ins(), 1);
    let stopped = world
        .keeper
        .lease(request(&key, &entry, Wait::DEFAULT))
        .await
        .unwrap_err();
    assert_eq!(stopped.code(), &codes::CREDENTIAL_REFUSED);

    // With a limit of two, and after a check that confirmed the credential,
    // one fresh sign-in is made when the system refuses it later.
    let world = World::new(STORED_HELD);
    let entry = Arc::new(AuthEntry {
        attempt_limit: 2,
        ..AuthEntry::new(ProviderName::new("fake").unwrap())
    });
    let lease = world
        .keeper
        .lease(request(&key, &entry, Wait::DEFAULT))
        .await
        .unwrap();
    *world.probe.outcome.lock().unwrap() = ProbeOutcome::SignedOut;
    world.keeper.report(&lease, ExpirySign::Refused).await;
    settle().await;
    assert_eq!(world.state(&key).await, SessionState::Valid);
    assert_eq!(world.fake.sign_ins(), 2);
    let again = world
        .keeper
        .lease(request(&key, &entry, Wait::DEFAULT))
        .await
        .unwrap();
    assert_eq!(again.epoch(), 2);
    world.keeper.report(&again, ExpirySign::Refused).await;
    settle().await;
    assert_eq!(world.state(&key).await, SessionState::NeedsPerson);
    assert_eq!(world.fake.sign_ins(), 2);
}

#[tokio::test]
async fn an_identity_service_that_is_down_fails_each_call_once_and_never_loops() {
    let world = World::new(STORED_RENEWING);
    let key = account_key("robot");
    let entry = fake_entry();
    world
        .keeper
        .lease(request(&key, &entry, Wait::DEFAULT))
        .await
        .unwrap();
    world.clock.advance(2 * HOUR);
    *world.fake.on_refresh.lock().unwrap() = Step::Unreachable;
    *world.fake.on_sign_in.lock().unwrap() = Step::Unreachable;

    // The renewal fails: the call that started it is told why, the session
    // is expired, and the rung is not tried again.
    let failed = world
        .keeper
        .lease(request(&key, &entry, Wait::DEFAULT))
        .await
        .unwrap_err();
    assert_eq!(failed.class(), FailureClass::Transient);
    assert_eq!(failed.code(), &codes::UPSTREAM_UNAVAILABLE);
    assert_eq!(world.state(&key).await, SessionState::Expired);
    assert_eq!((world.fake.refreshes(), world.fake.sign_ins()), (1, 1));

    // Each later call makes one attempt of its own with the stored
    // credential, and is told the same. A failure to reach the service is
    // not a refusal, so it does not use up the attempt limit.
    for attempt in 2..5 {
        let failed = world
            .keeper
            .lease(request(&key, &entry, Wait::DEFAULT))
            .await
            .unwrap_err();
        assert_eq!(failed.code(), &codes::UPSTREAM_UNAVAILABLE);
        assert_eq!(world.fake.sign_ins(), attempt);
    }
    *world.fake.on_sign_in.lock().unwrap() = Step::Works;
    let lease = world
        .keeper
        .lease(request(&key, &entry, Wait::DEFAULT))
        .await
        .unwrap();
    assert_eq!(lease.state(), SessionState::Valid);
}

#[tokio::test]
async fn saved_state_survives_a_restart_and_opens_for_its_owner_alone() {
    let world = World::new(PERSON_RENEWING);
    let key = person_key(1);
    let entry = fake_entry();
    world
        .keeper
        .sign_in(&key, Arc::clone(&entry), Some(person_id(1)))
        .await
        .unwrap();
    settle().await;
    let before = world
        .keeper
        .lease(request(&key, &entry, Wait::NoWait))
        .await
        .unwrap();
    let token = placed(&before)[0].1.clone();

    // A restart: nothing is held in memory, and nobody signs in again.
    let after = world.restarted();
    assert!(!after.worker.holds(before.session(), before.epoch()));
    assert!(!after.is_registered(&token));
    let lease = after
        .keeper
        .lease(request(&key, &entry, Wait::NoWait))
        .await
        .unwrap();
    assert_eq!(lease.epoch(), before.epoch());
    assert_eq!(placed(&lease)[0].1, token);
    assert_eq!(after.fake.sign_ins(), 1);
    assert!(after.is_registered(&token));
    assert!(after.kinds(&key).await.contains(&"restored"));

    // The saved state is sealed: its bytes do not hold the token, and it
    // opens under no other owner, kind, or item.
    let sealed = after.store.saved(lease.session()).await.unwrap().unwrap();
    let bytes = sealed.to_bytes();
    assert!(!bytes.windows(token.len()).any(|window| window == token.as_bytes()));
    let own = SealContext {
        workspace: key.workspace,
        owner: OwnerRef::of(&key.identity),
        kind: SealedKind::SessionState,
        item: ItemId::new(lease.session().to_string()).unwrap(),
    };
    assert!(after.vault.open(&own, &sealed).await.is_ok());
    let other_owner = SealContext {
        owner: OwnerRef::of(&person_key(2).identity),
        ..own.clone()
    };
    let refused = after.vault.open(&other_owner, &sealed).await.unwrap_err();
    assert_eq!(refused.code(), &a2m_vault::codes::SEALED_ITEM_MISMATCH);

    // Handed to the worker as another person's session, it is not held.
    let theirs = SessionRef {
        session: lease.session(),
        key: person_key(2),
        epoch: 1,
        home: None,
    };
    let outcome = after
        .worker
        .run(
            KeeperJob {
                session: theirs,
                entry: Arc::clone(&entry),
                work: Work::Restore(sealed),
            },
            after.person.as_ref(),
        )
        .await;
    assert!(matches!(outcome, WorkOutcome::Failed(_)), "{outcome:?}");

    // A session whose state was never saved does not survive: it is
    // expired, and the person signs in again.
    let unsaved = World::new(PERSON_RENEWING);
    let pinned = Arc::new(AuthEntry {
        movable: false,
        ..AuthEntry::new(ProviderName::new("fake").unwrap())
    });
    unsaved
        .keeper
        .sign_in(&key, Arc::clone(&pinned), Some(person_id(1)))
        .await
        .unwrap();
    settle().await;
    let again = unsaved.restarted();
    let gone = again
        .keeper
        .lease(request(&key, &pinned, Wait::NoWait))
        .await
        .unwrap_err();
    assert_eq!(gone.code(), &codes::NEEDS_LOGIN);
    assert_eq!(again.state(&key).await, SessionState::Expired);
}

#[tokio::test]
async fn the_timers_end_a_session_at_its_maximum_age_and_probe_only_while_wanted() {
    let world = World::new(STORED_RENEWING);
    let key = account_key("robot");
    let entry = Arc::new(AuthEntry {
        idle: Some(Duration::from_secs(1200)),
        absolute: Some(8 * HOUR),
        ..AuthEntry::new(ProviderName::new("fake").unwrap())
    });
    let settings = KeeperSettings::default();
    world
        .keeper
        .lease(request(&key, &entry, Wait::DEFAULT))
        .await
        .unwrap();

    // Probed at half the declared idle timeout, and not before.
    world.clock.advance(Duration::from_secs(599));
    world.keeper.tick().await;
    assert_eq!(world.probe.calls(), 0);
    world.clock.advance(Duration::from_secs(1));
    world.keeper.tick().await;
    world.keeper.tick().await;
    assert_eq!(world.probe.calls(), 1);

    // A pipeline that needs more time than the maximum age leaves stops at
    // once.
    let mut long = request(&key, &entry, Wait::NoWait);
    long.headroom = Some(8 * HOUR);
    let short = world.keeper.lease(long).await.unwrap_err();
    assert_eq!(short.code(), &codes::HEADROOM_SHORT);
    let mut brief = request(&key, &entry, Wait::NoWait);
    brief.headroom = Some(HOUR);
    world.keeper.lease(brief).await.unwrap();

    // The maximum age is the organization's: at eight hours the session
    // ends, whatever was renewed meanwhile.
    world.clock.advance(8 * HOUR);
    world.keeper.tick().await;
    assert_eq!(world.state(&key).await, SessionState::Expired);
    let probes = world.probe.calls();

    // Signed in again and then left alone: after the keep-alive window it
    // is no longer probed.
    world
        .keeper
        .lease(request(&key, &entry, Wait::DEFAULT))
        .await
        .unwrap();
    world.clock.advance(HOUR / 2);
    world.keeper.tick().await;
    assert_eq!(world.probe.calls(), probes + 1);
    let unwatched = Arc::new(AuthEntry {
        absolute: None,
        ..(*entry).clone()
    });
    world
        .keeper
        .lease(request(&key, &unwatched, Wait::DEFAULT))
        .await
        .unwrap();
    let probes = world.probe.calls();
    world.clock.advance(settings.keep_alive);
    world.keeper.tick().await;
    assert_eq!(world.probe.calls(), probes);
    assert_eq!(world.state(&key).await, SessionState::Valid);
}

#[tokio::test]
async fn work_that_was_interrupted_is_ended_and_not_left_waiting() {
    let world = World::new(STORED_RENEWING);
    let key = account_key("robot");
    let entry = fake_entry();
    world
        .keeper
        .lease(request(&key, &entry, Wait::DEFAULT))
        .await
        .unwrap();
    world.clock.advance(2 * HOUR);
    world.fake.hold_renewals();
    let run = {
        let keeper = world.keeper.clone();
        let req = request(&key, &entry, Wait::DEFAULT);
        tokio::spawn(async move { keeper.lease(req).await })
    };
    settle().await;
    assert_eq!(world.state(&key).await, SessionState::Renewing);

    // The process stops in the middle of the renewal. The one that starts
    // next finds the record in `renewing` with nobody working on it.
    let after = world.restarted();
    after.keeper.tick().await;
    assert_eq!(after.state(&key).await, SessionState::Renewing);
    after
        .clock
        .advance(KeeperSettings::default().work_limit);
    after.keeper.tick().await;
    assert_eq!(after.state(&key).await, SessionState::Expired);
    run.abort();
}
