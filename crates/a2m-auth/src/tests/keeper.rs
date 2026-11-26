//! The keeper's invariants: who may have a lease, one renewer, the ladder
//! climbed once, `needs_login` when nothing can renew, and saved state that
//! survives a restart and opens for its owner alone.

use std::sync::Arc;
use std::time::Duration;

use a2m_core::{FailureClass, Secret, SessionState};
use a2m_vault::{ItemId, OwnerRef, SealContext, SealedKind};

use super::support::{
    HOUR, LEAKED, PERSON_ONLY, PERSON_RENEWING, STORED_HELD, STORED_RENEWING, STORED_SIGNING_IN,
    Sink, Step, World, account_key, fake_entry, person_id, person_key, placed, reference, request,
    until,
};
use crate::{
    AUTOMATIC_SIGN_INS, AuthEntry, EndedBy, ExpirySign, KeeperJob, KeeperSettings, PersonAnswer,
    PersonAsk, PersonReason, ProviderName, RENEWALS, Returned, SessionCheckOutcome, SessionKey,
    SessionLease, SessionRef, SignInBy, Wait, Work, WorkOutcome, codes,
};

#[tokio::test]
async fn a_lease_is_refused_for_another_persons_session() {
    let world = World::new(PERSON_RENEWING).await;
    let key = person_key(1);
    let entry = fake_entry();
    world
        .person
        .answers(|_| PersonAnswer::Done(Returned::new()));
    world
        .keeper
        .sign_in(&key, Arc::clone(&entry), Some(person_id(1)))
        .await
        .unwrap();
    world.keeper.idle().await;
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
    let world = World::new(STORED_RENEWING).await;
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
    until(|| world.keeper.waiting() == 10 && world.fake.refreshes() == 1).await;
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
        kinds
            .iter()
            .filter(|kind| **kind == "renewal_started")
            .count(),
        1
    );
    // The count is made by the renewal's own task after its last write, so
    // it is read once that task has ended.
    world.keeper.idle().await;
    assert_eq!(world.logs_and_traces.metrics().counter(RENEWALS, &[]), 1);

    // The lease from before the renewal is stale, and says so.
    let mut sink = super::support::Sink::default();
    let stale = first.apply(&mut sink).unwrap_err();
    assert_eq!(stale.code(), &codes::LEASE_STALE);
    assert!(sink.placed.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_renewal_also_under_real_parallelism() {
    let world = World::new(STORED_RENEWING).await;
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

/// Two engines over one store: two keepers, each with its own waiters, meet
/// one expired session at once. The store's compare-and-set on the
/// session's state and epoch lets one of them start the renewal, whichever
/// it is: the one that signed the session in, or the other, whose runner
/// opens the saved state first. The other waits for it, and every run of
/// both is given the one renewed epoch. A
/// keeper hears of its own renewal at once, and of the other engine's at
/// its next tick, which wakes every waiting call to read the store again.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_engines_over_one_store_make_one_renewal() {
    let world = World::new(STORED_RENEWING).await;
    let other = world.second_engine();
    let key = account_key("robot");
    let entry = fake_entry();
    world
        .keeper
        .lease(request(&key, &entry, Wait::DEFAULT))
        .await
        .unwrap();
    assert_eq!(world.fake.sign_ins(), 1);

    world.clock.advance(2 * HOUR);
    world.fake.hold_renewals();
    let runs: Vec<_> = (0..10)
        .map(|at| {
            let keeper = if at % 2 == 0 {
                world.keeper.clone()
            } else {
                other.clone()
            };
            let req = request(&key, &entry, Wait::DEFAULT);
            tokio::spawn(async move { keeper.lease(req).await })
        })
        .collect();
    until(|| world.keeper.waiting() + other.waiting() == 10 && world.fake.refreshes() == 1).await;
    world.fake.release_renewals();
    // Which engine won the compare-and-set is the scheduler's choice. The
    // renewal's task ends after its last write, and only the first engine
    // has logs and traces, which counts the renewal when that engine made it.
    world.keeper.idle().await;
    other.idle().await;
    let first_renewed = match world.logs_and_traces.metrics().counter(RENEWALS, &[]) {
        1 => true,
        0 => false,
        counted => panic!("the first engine counted {counted} renewals"),
    };
    let waiter = if first_renewed { &other } else { &world.keeper };
    let (mine, theirs): (Vec<_>, Vec<_>) = runs
        .into_iter()
        .enumerate()
        .partition(|(at, _)| (at % 2 == 0) == first_renewed);
    // The renewer's own runs hear of its renewal at once, with no tick.
    for (_, run) in mine {
        assert_eq!(run.await.unwrap().unwrap().epoch(), 2);
    }
    // The other engine's runs wait until that engine's own tick, as its
    // tending task makes it.
    assert_eq!(waiter.waiting(), 5);
    waiter.tick().await;
    for (_, run) in theirs {
        let lease = tokio::time::timeout(Duration::from_secs(30), run)
            .await
            .expect("a run of one engine waited past the tick for the other's renewal")
            .unwrap()
            .unwrap();
        assert_eq!(lease.epoch(), 2);
    }
    assert_eq!((world.fake.refreshes(), world.fake.sign_ins()), (1, 1));
    let kinds = world.kinds(&key).await;
    assert_eq!(kinds.iter().filter(|kind| **kind == "renewed").count(), 1);
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == "renewal_started")
            .count(),
        1
    );
}

/// The engine that renews is not always the one that holds the session's
/// credentials: here the other engine signed the session in, and this one
/// meets it expired. Its runner opens the credentials from the saved state
/// and renews silently, as the engine that signed in would; the session is
/// never put to its person for want of credentials another runner holds.
#[tokio::test]
async fn an_engine_that_did_not_sign_in_renews_from_the_saved_state() {
    let world = World::new(STORED_RENEWING).await;
    let other = world.second_engine();
    let key = account_key("robot");
    let entry = fake_entry();
    world
        .keeper
        .lease(request(&key, &entry, Wait::DEFAULT))
        .await
        .unwrap();
    world.clock.advance(2 * HOUR);
    let lease = other
        .lease(request(&key, &entry, Wait::NoWait))
        .await
        .unwrap();
    assert_eq!(lease.epoch(), 2);
    assert_eq!((world.fake.refreshes(), world.fake.sign_ins()), (1, 1));
    assert!(!placed(&lease).is_empty());
    assert_eq!(world.state(&key).await, SessionState::Valid);
    let kinds = world.kinds(&key).await;
    assert!(!kinds.contains(&"prompted"), "{kinds:?}");
    assert!(world.person.asks.lock().unwrap().is_empty());
}

/// After a restart the runner holds nothing, and a session whose
/// credentials ran out meanwhile is renewed from its saved state, silently,
/// with no sign-in and no ask of its person.
#[tokio::test]
async fn a_restarted_engine_renews_an_expired_session_from_the_saved_state() {
    let world = World::on_disk(STORED_RENEWING).await;
    let key = account_key("robot");
    let entry = fake_entry();
    world
        .keeper
        .lease(request(&key, &entry, Wait::DEFAULT))
        .await
        .unwrap();
    world.keeper.idle().await;
    let world = world.restarted().await;
    world.clock.advance(2 * HOUR);
    let lease = world
        .keeper
        .lease(request(&key, &entry, Wait::NoWait))
        .await
        .unwrap();
    assert_eq!(lease.epoch(), 2);
    assert_eq!((world.fake.refreshes(), world.fake.sign_ins()), (1, 1));
    assert!(!placed(&lease).is_empty());
    assert_eq!(world.state(&key).await, SessionState::Valid);
    assert!(world.person.asks.lock().unwrap().is_empty());
}

#[tokio::test]
async fn ten_reports_of_one_sign_cause_one_check_and_a_stale_report_changes_nothing() {
    let world = World::new(STORED_RENEWING).await;
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

    // The session check says the session is gone, so the one re-check starts the
    // one renewal.
    *world.probe.outcome.lock().unwrap() = SessionCheckOutcome::SignedOut;
    for lease in &leases {
        world.keeper.report(lease, sign).await;
    }
    world.keeper.idle().await;
    assert_eq!(world.probe.calls(), 1);
    assert_eq!(world.fake.refreshes(), 1);
    assert_eq!(world.state(&key).await, SessionState::Valid);

    // The same leases report again: their epoch is no longer the session's.
    for lease in &leases {
        world.keeper.report(lease, sign).await;
    }
    world.keeper.idle().await;
    assert_eq!((world.probe.calls(), world.fake.refreshes()), (1, 1));

    // A sign on the new credentials that the session check does not confirm is one
    // check and no renewal.
    *world.probe.outcome.lock().unwrap() = SessionCheckOutcome::SignedIn;
    let fresh = world
        .keeper
        .lease(request(&key, &entry, Wait::DEFAULT))
        .await
        .unwrap();
    world.keeper.report(&fresh, sign).await;
    world.keeper.idle().await;
    assert_eq!((world.probe.calls(), world.fake.refreshes()), (2, 1));
    assert_eq!(world.state(&key).await, SessionState::Valid);
}

#[tokio::test]
async fn an_expired_session_with_no_renewal_path_gives_needs_login_to_every_call() {
    let world = World::new(PERSON_ONLY).await;
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
    world.keeper.idle().await;
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
    world.keeper.idle().await;
    assert_eq!(world.person.notices(), 1);
    assert_eq!((world.fake.sign_ins(), world.fake.refreshes()), (1, 0));

    // Interactive calls wait for the person, each up to its own wait. The
    // clock is the test's; the tick is what looks at it. After 250 seconds
    // the call that waits 200 has stopped and the call that waits 300 has
    // not.
    let wait_for = |seconds: u64| {
        let keeper = world.keeper.clone();
        let req = request(
            &key,
            &entry,
            Wait::Interactive(Duration::from_secs(seconds)),
        );
        tokio::spawn(async move { keeper.lease(req).await })
    };
    let (short, long) = (wait_for(200), wait_for(300));
    until(|| world.keeper.waiting() == 2).await;
    world.clock.advance(Duration::from_secs(250));
    world.keeper.tick().await;
    assert_eq!(
        short.await.unwrap().unwrap_err().code(),
        &codes::NEEDS_LOGIN
    );

    // The person signs in again: the call still waiting runs, and so does
    // every call after it.
    world
        .keeper
        .sign_in(&key, Arc::clone(&entry), Some(person_id(1)))
        .await
        .unwrap();
    let lease = long.await.unwrap().unwrap();
    assert_eq!(lease.epoch(), 2);
    assert_eq!(world.fake.sign_ins(), 2);

    // Ended on purpose: `needs_login`, the old lease places nothing, and
    // nothing is retried.
    world
        .keeper
        .revoke(&key, EndedBy::SessionOwner)
        .await
        .unwrap();
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
    let world = World::new(PERSON_RENEWING).await;
    let key = person_key(1);
    let entry = fake_entry();
    world
        .keeper
        .sign_in(&key, Arc::clone(&entry), Some(person_id(1)))
        .await
        .unwrap();
    world.keeper.idle().await;
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
    // The call returned on the record. The notice is sent by the renewal's
    // own task after it wrote the record, so it is read once that task has
    // ended.
    world.keeper.idle().await;
    assert!(matches!(
        world.person.asks.lock().unwrap()[0],
        PersonAsk::Notify {
            reason: PersonReason::SecondFactor
        }
    ));
}

#[tokio::test]
async fn a_refused_refresh_goes_to_the_person_and_is_never_tried_again() {
    let world = World::new(PERSON_RENEWING).await;
    let key = person_key(1);
    let entry = fake_entry();
    world
        .keeper
        .sign_in(&key, Arc::clone(&entry), Some(person_id(1)))
        .await
        .unwrap();
    world.keeper.idle().await;
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
    world.keeper.idle().await;
    assert_eq!(world.person.notices(), 1);
}

/// A sign-in a person asked for whose stored credential is refused counts
/// the attempt, so the next run's lease does not try the same credential
/// again by itself: a lockout would count that as a second failure.
#[tokio::test]
async fn a_refused_sign_in_a_person_asked_for_is_not_tried_again_by_a_lease() {
    let world = World::new(STORED_RENEWING).await;
    let key = account_key("robot");
    let entry = fake_entry();
    *world.fake.on_sign_in.lock().unwrap() = Step::Refused;
    world
        .keeper
        .sign_in(&key, Arc::clone(&entry), None)
        .await
        .unwrap();
    world.keeper.idle().await;
    assert_eq!(world.fake.sign_ins(), 1);
    assert_eq!(world.state(&key).await, SessionState::Absent);
    let stopped = world
        .keeper
        .lease(request(&key, &entry, Wait::DEFAULT))
        .await
        .unwrap_err();
    assert_eq!(stopped.code(), &codes::CREDENTIAL_REFUSED);
    world.keeper.idle().await;
    assert_eq!(world.fake.sign_ins(), 1);
}

#[tokio::test]
async fn a_refused_stored_credential_is_not_tried_again() {
    // At sign-in.
    let world = World::new(STORED_RENEWING).await;
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
    world.keeper.idle().await;
    assert_eq!(world.person.notices(), 1);

    // A program that asks for a sign-in does not get one: the same
    // credential would be sent again.
    let asked = world
        .keeper
        .sign_in(&key, Arc::clone(&entry), None)
        .await
        .unwrap_err();
    assert_eq!(asked.code(), &codes::CREDENTIAL_REFUSED);
    assert_eq!(world.fake.sign_ins(), 1);

    // The person looks at it and signs in themself: one more try, no more.
    world
        .keeper
        .sign_in_by(&key, Arc::clone(&entry), None, SignInBy::Person)
        .await
        .unwrap();
    world.keeper.idle().await;
    assert_eq!(world.fake.sign_ins(), 2);
    assert_eq!(world.state(&key).await, SessionState::Absent);
    *world.fake.on_sign_in.lock().unwrap() = Step::Works;
    world
        .keeper
        .sign_in_by(&key, Arc::clone(&entry), None, SignInBy::Person)
        .await
        .unwrap();
    world.keeper.idle().await;
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

/// A credential a person chose that is shorter than eight bytes is refused
/// by name before it is sealed: nothing is stored under its reference, and
/// it is registered with no redactor (decision 0338).
#[tokio::test]
async fn a_chosen_credential_of_one_byte_is_refused_and_nothing_is_stored() {
    let world = World::new(STORED_RENEWING).await;
    let key = person_key(1);
    let refused = world
        .credentials
        .put(
            &key.workspace,
            &key.identity,
            &reference("password"),
            &Secret::new("q"),
        )
        .await
        .unwrap_err();
    assert_eq!(refused.code().as_str(), "secret_too_short", "{refused}");
    assert_eq!(refused.class(), FailureClass::Input);
    assert!(
        !world
            .credentials
            .holds(&key.workspace, &key.identity, &reference("password"))
            .await
            .unwrap()
    );
    assert!(!world.is_registered("q"));
}

/// The count of a refused credential is the session's record's, so a
/// restart of the engine does not clear it: a program that asks again, and
/// every lease, is refused without sending. It is cleared when the person
/// stores the credential again, and the person's own sign-in is tried.
#[tokio::test]
async fn a_refused_stored_credential_is_not_sent_again_after_a_restart_until_the_person_acts() {
    let world = World::on_disk(STORED_RENEWING).await;
    let key = account_key("robot");
    let entry = Arc::new(
        AuthEntry::new(ProviderName::new("fake").unwrap())
            .credential("password", reference("password"))
            .published(),
    );
    world
        .store_credential(&key, "password", "secret-for-tests-wrong")
        .await;
    *world.fake.on_sign_in.lock().unwrap() = Step::Refused;
    world
        .keeper
        .sign_in(&key, Arc::clone(&entry), None)
        .await
        .unwrap();
    world.keeper.idle().await;
    assert_eq!(world.fake.sign_ins(), 1);

    // The engine stops when idle and starts again: a second keeper over the
    // same store. A program asks for the sign-in, and runs ask for the
    // session: nothing is sent.
    let world = world.restarted().await;
    for _ in 0..2 {
        let asked = world
            .keeper
            .sign_in(&key, Arc::clone(&entry), None)
            .await
            .unwrap_err();
        assert_eq!(asked.code(), &codes::CREDENTIAL_REFUSED);
        let leased = world
            .keeper
            .lease(request(&key, &entry, Wait::NoWait))
            .await
            .unwrap_err();
        assert_eq!(leased.code(), &codes::CREDENTIAL_REFUSED);
    }
    world.keeper.idle().await;
    assert_eq!(world.fake.sign_ins(), 1);
    // The record names the credential by the mark of its envelope, which
    // holds nothing of its value.
    let row = world.store.find(&key).await.unwrap().unwrap();
    let mark = row.attempted_with.unwrap().to_string();
    assert!(!mark.contains("secret-for-tests-wrong"));
    assert_ne!(
        mark,
        a2m_core::Digest::of(b"secret-for-tests-wrong").to_string()
    );

    // The person stores the credential again: it is tried once, and its
    // refusal is counted again, across the next restart too.
    world
        .store_credential(&key, "password", "secret-for-tests-still-wrong")
        .await;
    world
        .keeper
        .sign_in(&key, Arc::clone(&entry), None)
        .await
        .unwrap();
    world.keeper.idle().await;
    assert_eq!(world.fake.sign_ins(), 2);
    let world = world.restarted().await;
    let asked = world
        .keeper
        .sign_in(&key, Arc::clone(&entry), None)
        .await
        .unwrap_err();
    assert_eq!(asked.code(), &codes::CREDENTIAL_REFUSED);
    assert_eq!(world.fake.sign_ins(), 2);

    // The person signs in themself: it is tried, and holds.
    *world.fake.on_sign_in.lock().unwrap() = Step::Works;
    world
        .keeper
        .sign_in_by(&key, Arc::clone(&entry), None, SignInBy::Person)
        .await
        .unwrap();
    world.keeper.idle().await;
    assert_eq!(world.fake.sign_ins(), 3);
    assert_eq!(world.state(&key).await, SessionState::Valid);
}

/// A sign-in that sent the credential and got an answer no rule covers is
/// an attempt: at the limit the session waits for its person, and neither
/// later leases nor a program that asks send the credential again.
#[tokio::test]
async fn a_sign_in_whose_answer_nobody_can_read_is_counted_and_not_sent_again() {
    // By a lease.
    let world = World::new(STORED_RENEWING).await;
    let key = account_key("robot");
    let entry = fake_entry();
    *world.fake.on_sign_in.lock().unwrap() = Step::Unreadable;
    // The call that made the sign-in is told how it ended; the calls after
    // it, that the session waits for its person.
    for call in 0..3 {
        let stopped = world
            .keeper
            .lease(request(&key, &entry, Wait::NoWait))
            .await
            .unwrap_err();
        if call == 0 {
            assert_eq!(stopped.code(), &codes::UNEXPECTED_RESPONSE);
        } else {
            assert_eq!(stopped.class(), FailureClass::SessionExpired);
        }
        world.keeper.idle().await;
    }
    assert_eq!(world.fake.sign_ins(), 1);
    assert_eq!(world.state(&key).await, SessionState::NeedsPerson);
    let asked = world
        .keeper
        .sign_in(&key, Arc::clone(&entry), None)
        .await
        .unwrap_err();
    assert_eq!(asked.code(), &codes::UNEXPECTED_RESPONSE);
    world.keeper.idle().await;
    assert_eq!(world.fake.sign_ins(), 1);
    assert_eq!(world.person.notices(), 1);

    // By a program that asked: the leases after it do not send it again.
    let world = World::new(STORED_RENEWING).await;
    *world.fake.on_sign_in.lock().unwrap() = Step::Unreadable;
    world
        .keeper
        .sign_in(&key, Arc::clone(&entry), None)
        .await
        .unwrap();
    world.keeper.idle().await;
    for _ in 0..3 {
        world
            .keeper
            .lease(request(&key, &entry, Wait::NoWait))
            .await
            .unwrap_err();
    }
    world.keeper.idle().await;
    assert_eq!(world.fake.sign_ins(), 1);

    // In the renewal's full sign-in.
    let world = World::new(STORED_SIGNING_IN).await;
    let lease = world
        .keeper
        .lease(request(&key, &entry, Wait::NoWait))
        .await
        .unwrap();
    *world.fake.on_sign_in.lock().unwrap() = Step::Unreadable;
    *world.probe.outcome.lock().unwrap() = SessionCheckOutcome::SignedOut;
    world.keeper.report(&lease, ExpirySign::Refused).await;
    world.keeper.idle().await;
    assert_eq!(world.fake.sign_ins(), 2);
    for _ in 0..3 {
        world
            .keeper
            .lease(request(&key, &entry, Wait::NoWait))
            .await
            .unwrap_err();
    }
    world.keeper.idle().await;
    assert_eq!(world.fake.sign_ins(), 2);
    assert_eq!(world.state(&key).await, SessionState::NeedsPerson);
}

/// A system that keeps showing the sign of an expired session, which a
/// page's content can show on purpose, gets at most four sign-ins without
/// its person in an hour, counted on the keeper's clock. The fifth waits
/// for the person, however much later it is asked for, and the person's
/// own sign-in starts the count again. Sign-ins further apart are not
/// limited.
#[tokio::test]
async fn a_session_signed_in_again_too_often_without_its_person_waits_for_the_person() {
    let world = World::new(STORED_SIGNING_IN).await;
    let key = account_key("robot");
    let entry = fake_entry();
    *world.probe.outcome.lock().unwrap() = SessionCheckOutcome::SignedOut;
    let expire = |world: &World| {
        let keeper = world.keeper.clone();
        let entry = Arc::clone(&entry);
        let key = key.clone();
        async move {
            let lease = keeper.lease(request(&key, &entry, Wait::NoWait)).await?;
            keeper.report(&lease, ExpirySign::Refused).await;
            keeper.idle().await;
            Ok::<_, a2m_core::Failure>(())
        }
    };
    // The first sign-in, by a lease, then three renewals each a full
    // sign-in, ten minutes apart.
    for _ in 0..AUTOMATIC_SIGN_INS {
        expire(&world).await.unwrap();
        world.clock.advance(Duration::from_secs(600));
    }
    assert_eq!(world.fake.sign_ins(), AUTOMATIC_SIGN_INS);
    assert_eq!(world.state(&key).await, SessionState::NeedsPerson);
    let stopped = expire(&world).await.unwrap_err();
    assert_eq!(stopped.code(), &codes::NEEDS_LOGIN);

    // Hours later, nothing has changed: a program that asks is refused too.
    world.clock.advance(3 * HOUR);
    world.keeper.tick().await;
    world.keeper.idle().await;
    let asked = world
        .keeper
        .sign_in(&key, Arc::clone(&entry), None)
        .await
        .unwrap_err();
    assert_eq!(asked.code(), &codes::NEEDS_LOGIN);
    assert!(
        asked.message().contains("4 times within 60 minutes"),
        "{asked:?}"
    );
    assert_eq!(world.fake.sign_ins(), AUTOMATIC_SIGN_INS);

    // The person signs in themself, and the count starts again.
    world
        .keeper
        .sign_in_by(&key, Arc::clone(&entry), None, SignInBy::Person)
        .await
        .unwrap();
    world.keeper.idle().await;
    assert_eq!(world.state(&key).await, SessionState::Valid);
    for _ in 0..=AUTOMATIC_SIGN_INS {
        expire(&world).await.unwrap();
    }
    assert_eq!(world.fake.sign_ins(), 2 * AUTOMATIC_SIGN_INS + 1);
    assert_eq!(world.state(&key).await, SessionState::NeedsPerson);

    // Sign-ins twenty-five minutes apart are never limited.
    let world = World::new(STORED_SIGNING_IN).await;
    *world.probe.outcome.lock().unwrap() = SessionCheckOutcome::SignedOut;
    world
        .keeper
        .lease(request(&key, &entry, Wait::NoWait))
        .await
        .unwrap();
    for _ in 0..10 {
        world.clock.advance(Duration::from_secs(1500));
        expire(&world).await.unwrap();
    }
    assert_eq!(world.fake.sign_ins(), 11);
    assert_eq!(world.state(&key).await, SessionState::Valid);
}

#[tokio::test]
async fn a_held_credential_the_system_refuses_is_offered_once_and_no_more() {
    let world = World::new(STORED_HELD).await;
    let key = account_key("robot");
    let entry = fake_entry();
    let lease = world
        .keeper
        .lease(request(&key, &entry, Wait::DEFAULT))
        .await
        .unwrap();
    // The system refuses it, and the session check agrees. The credential was never
    // confirmed, and the limit is one attempt: it is not offered again.
    *world.probe.outcome.lock().unwrap() = SessionCheckOutcome::SignedOut;
    world.keeper.report(&lease, ExpirySign::Refused).await;
    world.keeper.idle().await;
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
    let world = World::new(STORED_HELD).await;
    let entry = Arc::new(AuthEntry {
        attempt_limit: 2,
        ..AuthEntry::new(ProviderName::new("fake").unwrap()).published()
    });
    let lease = world
        .keeper
        .lease(request(&key, &entry, Wait::DEFAULT))
        .await
        .unwrap();
    *world.probe.outcome.lock().unwrap() = SessionCheckOutcome::SignedOut;
    world.keeper.report(&lease, ExpirySign::Refused).await;
    world.keeper.idle().await;
    assert_eq!(world.state(&key).await, SessionState::Valid);
    assert_eq!(world.fake.sign_ins(), 2);
    let again = world
        .keeper
        .lease(request(&key, &entry, Wait::DEFAULT))
        .await
        .unwrap();
    assert_eq!(again.epoch(), 2);
    world.keeper.report(&again, ExpirySign::Refused).await;
    world.keeper.idle().await;
    assert_eq!(world.state(&key).await, SessionState::NeedsPerson);
    assert_eq!(world.fake.sign_ins(), 2);
}

#[tokio::test]
async fn an_identity_service_that_is_down_fails_each_call_once_and_never_loops() {
    let world = World::new(STORED_RENEWING).await;
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
async fn an_unreachable_key_store_keeps_sessions_in_memory_and_restores_none_until_it_answers() {
    // Section 224: with the key store unreachable, sessions already in
    // memory keep working; none can be restored or saved; a call that
    // needs one fails by name; it recovers by itself.
    let world = World::on_disk(PERSON_RENEWING).await;
    let key = person_key(1);
    let entry = fake_entry();
    world
        .keeper
        .sign_in(&key, Arc::clone(&entry), Some(person_id(1)))
        .await
        .unwrap();
    world.keeper.idle().await;
    let before = world
        .keeper
        .lease(request(&key, &entry, Wait::NoWait))
        .await
        .unwrap();
    let token = placed(&before)[0].1.clone();

    world.keys_unreachable(true);
    let held = world
        .keeper
        .lease(request(&key, &entry, Wait::NoWait))
        .await
        .unwrap();
    assert_eq!(placed(&held)[0].1, token);

    // After a restart, the key store stops answering before the session is
    // back in memory. The saved session cannot be opened: the call fails by
    // name, and nothing is signed in or thrown away in its place. (A start
    // while the key store is unreachable leaves the kernel degraded, which
    // is the kernel's own test.)
    world.keys_unreachable(false);
    let after = world.restarted().await;
    after.keys_unreachable(true);
    let refused = after
        .keeper
        .lease(request(&key, &entry, Wait::NoWait))
        .await
        .unwrap_err();
    assert_eq!(refused.class(), FailureClass::Busy, "{refused:?}");
    assert_eq!(
        refused.code(),
        &a2m_vault::codes::KEY_STORE_UNREACHABLE,
        "{refused:?}"
    );
    after.keeper.idle().await;
    assert_eq!(after.fake.sign_ins(), 1);
    assert!(after.store.saved(before.session()).await.unwrap().is_some());

    // The key store answers again: the next call restores the same session.
    after.keys_unreachable(false);
    let restored = after
        .keeper
        .lease(request(&key, &entry, Wait::NoWait))
        .await
        .unwrap();
    assert_eq!(restored.session(), before.session());
    assert_eq!(placed(&restored)[0].1, token);
    assert_eq!(after.fake.sign_ins(), 1);
}

#[tokio::test]
async fn saved_state_survives_a_restart_and_opens_for_its_owner_alone() {
    let world = World::on_disk(PERSON_RENEWING).await;
    let key = person_key(1);
    let entry = fake_entry();
    world
        .keeper
        .sign_in(&key, Arc::clone(&entry), Some(person_id(1)))
        .await
        .unwrap();
    world.keeper.idle().await;
    let before = world
        .keeper
        .lease(request(&key, &entry, Wait::NoWait))
        .await
        .unwrap();
    let token = placed(&before)[0].1.clone();

    // A restart: nothing is held in memory, and nobody signs in again.
    let after = world.restarted().await;
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
    assert!(
        !bytes
            .windows(token.len())
            .any(|window| window == token.as_bytes())
    );
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
    let unsaved = World::on_disk(PERSON_RENEWING).await;
    let pinned = Arc::new(AuthEntry {
        movable: false,
        ..AuthEntry::new(ProviderName::new("fake").unwrap()).published()
    });
    unsaved
        .keeper
        .sign_in(&key, Arc::clone(&pinned), Some(person_id(1)))
        .await
        .unwrap();
    unsaved.keeper.idle().await;
    let again = unsaved.restarted().await;
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
    let world = World::new(STORED_RENEWING).await;
    let key = account_key("robot");
    let entry = Arc::new(AuthEntry {
        idle: Some(Duration::from_secs(1200)),
        absolute: Some(8 * HOUR),
        ..AuthEntry::new(ProviderName::new("fake").unwrap()).published()
    });
    let settings = KeeperSettings::default();
    world
        .keeper
        .lease(request(&key, &entry, Wait::DEFAULT))
        .await
        .unwrap();

    // checked at half the declared idle timeout, and not before.
    world.clock.advance(Duration::from_secs(599));
    world.keeper.tick().await;
    world.keeper.idle().await;
    assert_eq!(world.probe.calls(), 0);
    world.clock.advance(Duration::from_secs(1));
    world.keeper.tick().await;
    world.keeper.tick().await;
    world.keeper.idle().await;
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
    // is no longer checked.
    world
        .keeper
        .lease(request(&key, &entry, Wait::DEFAULT))
        .await
        .unwrap();
    world.clock.advance(HOUR / 2);
    world.keeper.tick().await;
    world.keeper.idle().await;
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
    world.keeper.idle().await;
    assert_eq!(world.probe.calls(), probes);
    assert_eq!(world.state(&key).await, SessionState::Valid);
}

#[tokio::test]
async fn a_probe_that_could_not_be_made_changes_nothing_and_a_probes_lease_never_signs_in() {
    let world = World::new(STORED_RENEWING).await;
    let key = account_key("robot");
    let entry = Arc::new(AuthEntry {
        idle: Some(Duration::from_secs(1200)),
        ..AuthEntry::new(ProviderName::new("fake").unwrap()).published()
    });

    // Before any sign-in a session check's lease is refused, and starts none.
    let mut probing = request(&key, &entry, Wait::DEFAULT);
    probing.purpose = crate::Purpose::Probe;
    let refused = world.keeper.lease(probing.clone()).await.unwrap_err();
    assert_eq!(refused.code(), &codes::NEEDS_LOGIN);
    assert_eq!(world.fake.sign_ins(), 0);

    world
        .keeper
        .lease(request(&key, &entry, Wait::DEFAULT))
        .await
        .unwrap();
    assert_eq!(world.fake.sign_ins(), 1);

    // A session check that answers nothing leaves the session valid, unchecked, and
    // is not made again until a whole interval later.
    *world.probe.outcome.lock().unwrap() = SessionCheckOutcome::Unknown;
    world.clock.advance(Duration::from_secs(600));
    world.keeper.tick().await;
    assert_eq!(world.probe.calls(), 1);
    assert_eq!(world.state(&key).await, SessionState::Valid);
    world.keeper.tick().await;
    assert_eq!(world.probe.calls(), 1);
    world.clock.advance(Duration::from_secs(600));
    world.keeper.tick().await;
    assert_eq!(world.probe.calls(), 2);
    assert_eq!(world.state(&key).await, SessionState::Valid);
    assert_eq!(world.fake.refreshes(), 0);

    // Once it is signed in, a session check's lease is given, and is no use: the
    // session's last use stays the run's.
    world.keeper.lease(probing).await.unwrap();
    assert_eq!(world.fake.sign_ins(), 1);
}

#[tokio::test]
async fn work_that_was_interrupted_is_ended_and_not_left_waiting() {
    let world = World::on_disk(STORED_RENEWING).await;
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
    until(|| world.fake.refreshes() == 1).await;
    assert_eq!(world.state(&key).await, SessionState::Renewing);

    // The process stops in the middle of the renewal. The one that starts
    // next finds the record in `renewing` with nobody working on it.
    run.abort();
    let after = world.restarted().await;
    after.keeper.tick().await;
    assert_eq!(after.state(&key).await, SessionState::Renewing);
    after.clock.advance(KeeperSettings::default().work_limit);
    after.keeper.tick().await;
    assert_eq!(after.state(&key).await, SessionState::Expired);
}

/// Lets every task that is ready run once, a few times over. Used only to
/// show that a task has not finished: one that could finish would, since
/// nothing it waits for here is on another thread.
async fn give_way() {
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
}

#[tokio::test]
async fn a_call_returns_on_the_record_and_idle_waits_for_what_the_work_does_after_it() {
    let world = World::new(PERSON_RENEWING).await;
    let key = person_key(1);
    let entry = fake_entry();
    world
        .keeper
        .sign_in(&key, Arc::clone(&entry), Some(person_id(1)))
        .await
        .unwrap();
    world.keeper.idle().await;
    world.clock.advance(2 * HOUR);
    *world.fake.on_refresh.lock().unwrap() = Step::WantsSecondFactor;

    // The notice to the person is held. The call is still answered: it is
    // decided by the session's record, which the renewal wrote before it
    // went to tell the person.
    world.person.gate.hold();
    let stopped = world
        .keeper
        .lease(request(&key, &entry, Wait::NoWait))
        .await
        .unwrap_err();
    assert_eq!(stopped.code(), &codes::NEEDS_MFA);
    assert_eq!(world.state(&key).await, SessionState::NeedsPerson);
    assert_eq!(world.person.notices(), 0);

    // The keeper is not idle while the notice is on its way, and is idle
    // once it has been given.
    let idle = {
        let keeper = world.keeper.clone();
        tokio::spawn(async move { keeper.idle().await })
    };
    give_way().await;
    assert!(!idle.is_finished());
    world.person.gate.release();
    idle.await.unwrap();
    assert_eq!(world.person.notices(), 1);
}

#[tokio::test]
async fn work_that_overlaps_on_one_session_keeps_it_busy_until_the_last_ends() {
    let world = World::new(STORED_RENEWING).await;
    let key = account_key("robot");
    let entry = fake_entry();
    let lease = world
        .keeper
        .lease(request(&key, &entry, Wait::DEFAULT))
        .await
        .unwrap();

    // A routine session check is in flight when a driver reports a sign on the same
    // session, which starts a re-check: two pieces of work at once.
    world.probe.gate.hold();
    world.clock.advance(HOUR / 2);
    let tick = {
        let keeper = world.keeper.clone();
        tokio::spawn(async move { keeper.tick().await })
    };
    until(|| world.probe.calls() == 1).await;
    world.keeper.report(&lease, ExpirySign::Refused).await;
    until(|| world.probe.calls() == 2).await;

    // The routine session check ends. The re-check has not, so the keeper is not
    // idle yet.
    world.probe.gate.release_one();
    tick.await.unwrap();
    let idle = {
        let keeper = world.keeper.clone();
        tokio::spawn(async move { keeper.idle().await })
    };
    give_way().await;
    assert!(!idle.is_finished());
    assert_eq!(world.state(&key).await, SessionState::Suspect);

    world.probe.gate.release();
    idle.await.unwrap();
    assert_eq!(world.state(&key).await, SessionState::Valid);
    assert_eq!(world.fake.refreshes(), 0);
}

#[tokio::test]
async fn a_call_that_is_cancelled_while_it_waits_is_no_longer_counted() {
    let world = World::new(PERSON_ONLY).await;
    let key = person_key(1);
    let entry = fake_entry();
    world
        .keeper
        .sign_in(&key, Arc::clone(&entry), Some(person_id(1)))
        .await
        .unwrap();
    world.keeper.idle().await;
    world.clock.advance(2 * HOUR);
    let waiting = {
        let keeper = world.keeper.clone();
        let req = request(&key, &entry, Wait::DEFAULT);
        tokio::spawn(async move { keeper.lease(req).await })
    };
    until(|| world.keeper.waiting() == 1).await;
    world.keeper.idle().await;
    assert_eq!(world.state(&key).await, SessionState::NeedsPerson);
    // The call has seen that a person is wanted and waits for one.
    until(|| world.keeper.waiting() == 1).await;
    give_way().await;
    assert_eq!(world.keeper.waiting(), 1);

    // The caller goes away.
    waiting.abort();
    assert!(waiting.await.unwrap_err().is_cancelled());
    assert_eq!(world.keeper.waiting(), 0);
}

/// A person's session, signed in, saved, and then met by an engine that
/// was restarted: the lease from before the restart, and the new engine.
async fn restarted_with_a_saved_session(key: &SessionKey) -> (SessionLease, World) {
    let world = World::on_disk(PERSON_RENEWING).await;
    let entry = fake_entry();
    world
        .keeper
        .sign_in(key, Arc::clone(&entry), Some(person_id(1)))
        .await
        .unwrap();
    world.keeper.idle().await;
    let before = world
        .keeper
        .lease(request(key, &entry, Wait::NoWait))
        .await
        .unwrap();
    (before, world.restarted().await)
}

fn stale(lease: &SessionLease) -> bool {
    let mut sink = Sink::default();
    let refused = lease.apply(&mut sink);
    assert!(refused.is_err() || !sink.placed.is_empty());
    refused.is_err_and(|failure| failure.code() == &codes::LEASE_STALE) && sink.placed.is_empty()
}

/// Audit finding A-1. Two calls restore a session after a restart; the
/// second is still on its way to the worker when the owner revokes. Each
/// step is let through by the test, so the order is the one that went wrong.
#[tokio::test]
async fn a_restore_that_ends_after_a_revoke_brings_nothing_back() {
    let key = person_key(1);
    let entry = fake_entry();
    let (before, world) = restarted_with_a_saved_session(&key).await;

    // The first call has read the saved state, and its restore is held on
    // the way to the worker.
    world.relay.restores.hold();
    let slow = {
        let keeper = world.keeper.clone();
        let req = request(&key, &entry, Wait::NoWait);
        tokio::spawn(async move { keeper.lease(req).await })
    };
    until(|| world.relay.restores_seen() == 1).await;

    // A second call restores, is given a lease, and uses it.
    world.relay.restores.open();
    let lease = world
        .keeper
        .lease(request(&key, &entry, Wait::NoWait))
        .await
        .unwrap();
    assert_eq!(lease.epoch(), before.epoch());
    assert_eq!(placed(&lease).len(), 1);

    // The owner ends the session. The lease places nothing from here on.
    world
        .keeper
        .revoke(&key, EndedBy::SessionOwner)
        .await
        .unwrap();
    assert!(stale(&lease));

    // The first call's restore now reaches the worker.
    world.relay.restores.release_one();
    let late = slow.await.unwrap().unwrap_err();
    assert_eq!(late.code(), &codes::NEEDS_LOGIN);
    world.keeper.idle().await;

    // Nothing came back: the worker holds no credentials of the revoked
    // session, and the lease from before the revoke still places nothing.
    assert!(!world.worker.holds(lease.session(), lease.epoch()));
    assert!(stale(&lease));
    assert_eq!(world.state(&key).await, SessionState::Revoked);
    assert!(!world.kinds(&key).await.ends_with(&["restored"]));
}

/// The keeper's own end of finding A-1: the sign-out never reaches the
/// worker, so the worker was not told that the session ended. The keeper
/// sees that the session moved on while its restore ran, and has the worker
/// drop what the restore brought back.
#[tokio::test]
async fn a_restore_is_undone_when_the_session_moved_on_while_it_ran() {
    let key = person_key(1);
    let entry = fake_entry();
    let (before, world) = restarted_with_a_saved_session(&key).await;
    world.relay.lose_sign_outs();

    world.relay.restores.hold();
    let slow = {
        let keeper = world.keeper.clone();
        let req = request(&key, &entry, Wait::NoWait);
        tokio::spawn(async move { keeper.lease(req).await })
    };
    until(|| world.relay.restores_seen() == 1).await;
    world
        .keeper
        .revoke(&key, EndedBy::SessionOwner)
        .await
        .unwrap();

    world.relay.restores.release_one();
    let late = slow.await.unwrap().unwrap_err();
    assert_eq!(late.code(), &codes::NEEDS_LOGIN);
    world.keeper.idle().await;
    assert!(!world.worker.holds(before.session(), before.epoch()));
    assert!(!world.kinds(&key).await.contains(&"restored"));
}

async fn run(
    world: &World,
    key: &SessionKey,
    lease: &SessionLease,
    epoch: u64,
    work: Work,
) -> WorkOutcome {
    let job = KeeperJob {
        session: SessionRef {
            session: lease.session(),
            key: key.clone(),
            epoch,
            home: None,
        },
        entry: fake_entry(),
        work,
    };
    world.worker.run(job, world.person.as_ref()).await
}

/// The worker's end of findings A-1 and A-2. An end names the epoch it
/// ends: it leaves newer credentials alone, and nothing at or below it is
/// held again.
#[tokio::test]
async fn the_worker_keeps_what_is_newer_than_an_end_and_never_holds_what_ended() {
    let world = World::new(PERSON_RENEWING).await;
    let key = person_key(1);
    let entry = fake_entry();
    world
        .keeper
        .sign_in(&key, Arc::clone(&entry), Some(person_id(1)))
        .await
        .unwrap();
    world.keeper.idle().await;
    let lease = world
        .keeper
        .lease(request(&key, &entry, Wait::NoWait))
        .await
        .unwrap();
    assert_eq!(lease.epoch(), 1);
    let saved = world.store.saved(lease.session()).await.unwrap().unwrap();

    // An end that names an earlier epoch arrives late. The credentials held
    // now are newer, and they stay.
    for late in [Work::Forget, Work::SignOut] {
        let outcome = run(&world, &key, &lease, 0, late).await;
        assert!(matches!(outcome, WorkOutcome::Ended), "{outcome:?}");
        assert!(world.worker.holds(lease.session(), 1));
        assert_eq!(placed(&lease).len(), 1);
    }

    // The session ends, as a revoke ends it: the record is at epoch 2.
    let outcome = run(&world, &key, &lease, 2, Work::SignOut).await;
    assert!(matches!(outcome, WorkOutcome::Ended), "{outcome:?}");
    assert!(stale(&lease));

    // A restore of what was saved at epoch 1 arrives after the end.
    let outcome = run(&world, &key, &lease, 1, Work::Restore(saved)).await;
    assert!(matches!(outcome, WorkOutcome::Ended), "{outcome:?}");
    assert!(!world.worker.holds(lease.session(), 1));
    assert!(stale(&lease));

    // The next sign-in holds at the next epoch, as before.
    let outcome = run(&world, &key, &lease, 2, Work::SignIn).await;
    assert!(
        matches!(outcome, WorkOutcome::Held { epoch: 3, .. }),
        "{outcome:?}"
    );
    assert!(world.worker.holds(lease.session(), 3));
}

/// A renewal is in flight when the owner revokes. What the renewal then
/// holds is dropped by the epoch it was held at, and nothing is kept.
#[tokio::test]
async fn work_that_ends_after_a_revoke_keeps_nothing() {
    let world = World::new(STORED_RENEWING).await;
    let key = account_key("svc");
    let entry = fake_entry();
    let lease = world
        .keeper
        .lease(request(&key, &entry, Wait::NoWait))
        .await
        .unwrap();
    world.keeper.idle().await;

    world.fake.hold_renewals();
    world.clock.advance(HOUR);
    let waiting = {
        let keeper = world.keeper.clone();
        let req = request(&key, &entry, Wait::NoWait);
        tokio::spawn(async move { keeper.lease(req).await })
    };
    until(|| world.fake.refreshes() == 1).await;
    world
        .keeper
        .revoke(&key, EndedBy::SessionOwner)
        .await
        .unwrap();
    world.fake.release_renewals();
    assert!(waiting.await.unwrap().is_err());
    world.keeper.idle().await;

    assert_eq!(world.state(&key).await, SessionState::Revoked);
    for epoch in 0..4 {
        assert!(!world.worker.holds(lease.session(), epoch), "{epoch}");
    }
    assert!(stale(&lease));
}

/// Audit finding A-3. A provider that breaks its contract puts a secret it
/// registered into its failure. The failure is redacted before it leaves
/// the worker, so the call is not given the secret.
#[tokio::test]
async fn a_providers_failure_is_redacted_before_it_leaves_the_worker() {
    let world = World::new(STORED_RENEWING).await;
    let key = account_key("svc");
    let entry = fake_entry();
    *world.fake.on_sign_in.lock().unwrap() = Step::Leaks;
    let failure = world
        .keeper
        .lease(request(&key, &entry, Wait::NoWait))
        .await
        .unwrap_err();
    assert_eq!(failure.code(), &codes::UPSTREAM_UNAVAILABLE);
    for text in [failure.message(), failure.next_step()] {
        assert!(!text.contains(LEAKED), "{text}");
        assert!(text.contains(a2m_core::REDACTED), "{text}");
    }
}

/// What a session check that leases through the keeper holds of it.
#[derive(Default)]
struct Leasing {
    strong: std::sync::OnceLock<crate::Keeper>,
    weak: std::sync::OnceLock<crate::WeakKeeper>,
}

impl crate::RunSessionCheck for Leasing {
    fn check_session<'a>(
        &'a self,
        _session: &'a SessionRef,
    ) -> crate::BoxFuture<'a, SessionCheckOutcome> {
        Box::pin(async move {
            let keeper = self
                .strong
                .get()
                .cloned()
                .or_else(|| self.weak.get().and_then(crate::WeakKeeper::upgrade));
            if keeper.is_some() {
                SessionCheckOutcome::SignedIn
            } else {
                SessionCheckOutcome::Unknown
            }
        })
    }
}

struct Nobody;

impl crate::AskPerson for Nobody {
    fn ask<'a>(
        &'a self,
        _session: &'a SessionRef,
        _ask: PersonAsk,
    ) -> crate::BoxFuture<'a, PersonAnswer> {
        Box::pin(async { PersonAnswer::Unreachable })
    }
}

/// A keeper whose work holds a session check that holds the keeper, as one that
/// leases through it must. Held weakly, the keeper is freed when the
/// engine lets go of it; held strongly, the cycle keeps it.
async fn engine_with_session_check(weakly: bool) -> crate::WeakKeeper {
    use a2m_ledger::{Store, StoreConfig};
    use a2m_logs::ManualClock;
    use a2m_vault::{MemoryKeyStore, Vault};

    let clock = Arc::new(ManualClock::starting_at(
        a2m_core::Timestamp::from_unix_millis(super::support::START),
    ));
    let ledger = Store::open(StoreConfig::in_memory().with_clock(clock.clone()))
        .await
        .unwrap();
    let vault = Arc::new(Vault::new(
        Arc::new(MemoryKeyStore::new()),
        Arc::new(ledger.key_records()),
    ));
    ledger.attach(Arc::clone(&vault)).await.unwrap();
    let store = crate::SessionStore::new(ledger);
    let providers = crate::ProviderSet::neutral();
    let worker = Arc::new(crate::KeeperWorker::new(
        providers.clone(),
        Arc::clone(&vault),
        Arc::new(crate::StoredCredentials::new(store.clone(), vault)),
        clock.clone(),
    ));
    let probe = Arc::new(Leasing::default());
    let work = Arc::new(crate::InProcessWork::new(
        Arc::clone(&worker),
        Arc::new(Nobody),
        Arc::clone(&probe) as Arc<dyn crate::RunSessionCheck>,
    ));
    let keeper = crate::Keeper::builder(store, providers, work, clock)
        .local_worker(worker)
        .build();
    if weakly {
        assert!(probe.weak.set(keeper.downgrade()).is_ok());
    } else {
        assert!(probe.strong.set(keeper.clone()).is_ok());
    }
    let session = SessionRef {
        session: a2m_core::SessionId::from_parts(
            a2m_core::Timestamp::from_unix_millis(super::support::START),
            [1; 10],
        )
        .unwrap(),
        key: account_key("robot"),
        epoch: 0,
        home: None,
    };
    // The session check reaches the keeper while the engine holds it.
    assert_eq!(
        crate::RunSessionCheck::check_session(probe.as_ref(), &session).await,
        SessionCheckOutcome::SignedIn
    );
    keeper.idle().await;
    keeper.downgrade()
}

#[tokio::test]
async fn a_closed_engines_keeper_is_freed_when_its_probe_holds_it_weakly() {
    let freed = engine_with_session_check(true).await;
    assert!(freed.upgrade().is_none());
    // The shape the check found in a test: a session check that holds the keeper
    // itself, held by the keeper's own work, is never dropped.
    let kept = engine_with_session_check(false).await;
    assert!(kept.upgrade().is_some());
}
