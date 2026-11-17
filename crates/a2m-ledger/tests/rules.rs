//! The nine rules of spec section 208, each broken on purpose and each
//! refused. No application is involved: the tests stand where the engine and
//! the policy engine will stand, and a second connection to the database
//! file stands where any other program would.

#![allow(
    clippy::unwrap_used,
    reason = "this file holds only tests; clippy.toml allows unwrap in tests, which it tells by the #[test] attribute, and the helpers the tests share do not carry it"
)]

mod common;

use a2m_core::{EffectClass, Epoch, FailureClass, RecordState, RunState};
use a2m_ledger::{
    Acted, Durability, Fence, Opened, ReadBack, Reconciled, Settled, WriteClearance, codes,
};
use common::*;

/// Rule 1. Intent before action.
#[tokio::test]
async fn rule_1_a_record_is_acted_only_after_its_intent_is_stored() {
    let folder = Folder::new("rule1");
    let store = on_disk(&folder).await;
    let fence = running(&store, 1, &["a", "b"]).await;
    let raw = folder.raw();

    // No call marks a record acted without a token, so the rule is broken
    // with a statement of our own: the store refuses it.
    let refused = raw
        .execute(
            "UPDATE run_records SET state = 'acted' WHERE record_seq = 0",
            [],
        )
        .unwrap_err();
    assert!(
        refused
            .to_string()
            .contains("acted only after its intent is stored"),
        "{refused}"
    );

    // When the token is returned, the intent is in the file already.
    lock(&store, fence, 0).await.unwrap();
    let token = intent(&store, fence, 0, "a").await.unwrap();
    let on_disk: String = raw
        .query_row(
            "SELECT state FROM run_records WHERE record_seq = 0",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(on_disk, "intent");
    assert_eq!(token.record(), record(&fence, 0));
    assert_eq!(token.intended(), &intended("a"));
    assert_eq!(token.epoch(), fence.epoch);

    // A token from a lease that ended changes nothing.
    lock(&store, fence, 1).await.unwrap();
    let late = intent(&store, fence, 1, "b").await.unwrap();
    let acted = store
        .write(Durability::Synced, move |tx| {
            tx.ledger().mark_acted(token, Acted::default())
        })
        .await
        .unwrap();
    assert_eq!(acted.record(), record(&fence, 0));
    assert_eq!(state_of(&store, fence, 0).await, RecordState::Acted);

    store
        .write(Durability::Synced, move |tx| {
            tx.ledger().new_epoch(fence.run)
        })
        .await
        .unwrap();
    let lost = store
        .write(Durability::Synced, move |tx| {
            tx.ledger().mark_acted(late, Acted::default())
        })
        .await
        .unwrap_err();
    assert_eq!(lost.code(), &codes::RUNNER_LOST);
    assert_eq!(state_of(&store, fence, 1).await, RecordState::Intent);
}

/// Rule 2. Proof before verified.
#[tokio::test]
async fn rule_2_a_record_is_verified_only_with_a_matching_read_back_and_evidence() {
    let folder = Folder::new("rule2");
    let store = on_disk(&folder).await;
    let fence = running(&store, 1, &["a"]).await;
    lock(&store, fence, 0).await.unwrap();
    let token = intent(&store, fence, 0, "a").await.unwrap();
    let acted = store
        .write(Durability::Synced, move |tx| {
            tx.ledger().mark_acted(token, Acted::default())
        })
        .await
        .unwrap();
    let proof = evidence(&store, "what was read").await;

    // A read-back that differs is refused, and the record stays acted.
    let differs = store
        .write(Durability::Synced, move |tx| {
            let read = ReadBack {
                digest: intended("something else"),
                after: None,
                evidence: proof,
            };
            tx.ledger().mark_verified(&acted, read).map(|()| acted)
        })
        .await
        .unwrap_err();
    assert_eq!(differs.class(), FailureClass::Mismatch);
    assert_eq!(differs.code(), &codes::READ_BACK_DIFFERS);
    assert_eq!(state_of(&store, fence, 0).await, RecordState::Acted);

    // Without a read-back and evidence the store itself refuses.
    let raw = folder.raw();
    let refused = raw
        .execute(
            "UPDATE run_records SET state = 'verified' WHERE record_seq = 0",
            [],
        )
        .unwrap_err();
    assert!(
        refused
            .to_string()
            .contains("matching read-back and its evidence"),
        "{refused}"
    );

    // Evidence that names no blob is refused too.
    let nothing = a2m_ledger::BlobRef::from_digest(a2m_core::Digest::of(b"no such blob"));
    let unproven = store
        .write(Durability::Synced, move |tx| {
            // The token was consumed by the failed call above; the record is
            // settled through reconcile, which asks the same proof.
            let read = ReadBack {
                digest: intended("a"),
                after: None,
                evidence: nothing,
            };
            tx.ledger()
                .settle(&fence, record(&fence, 0), Reconciled::Landed(read))
        })
        .await
        .unwrap_err();
    assert_eq!(unproven.code(), &codes::LEDGER_STATE_UNEXPECTED);

    // With both, it is verified, and its lock is released in the same step.
    let settled = store
        .write(Durability::Synced, move |tx| {
            let read = ReadBack {
                digest: intended("a"),
                after: Some(proof),
                evidence: proof,
            };
            tx.ledger()
                .settle(&fence, record(&fence, 0), Reconciled::Landed(read))
        })
        .await
        .unwrap();
    assert_eq!(settled, Settled::Verified);
    assert_eq!(state_of(&store, fence, 0).await, RecordState::Verified);
    let locks = store
        .read(move |tx| tx.ledger().locks(fence.run))
        .await
        .unwrap();
    assert!(locks.is_empty());
    let after: Option<String> = raw
        .query_row(
            "SELECT evidence_ref FROM run_records WHERE record_seq = 0",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(after, Some(proof.to_string()));
}

/// Rule 3. Approval bound to the diff.
#[tokio::test]
async fn rule_3_a_write_run_starts_only_with_a_clearance_for_its_diff() {
    let store = memory().await;
    let fence = previewed(&store, 1, &["a"]).await;

    // Not through the plain state change.
    let plain = store
        .write(Durability::Synced, move |tx| {
            tx.ledger().set_state(&fence, RunState::Running, None)
        })
        .await
        .unwrap_err();
    assert_eq!(
        (plain.class(), plain.code()),
        (FailureClass::Policy, &codes::APPROVAL_MISSING)
    );

    // Not with an approval for another diff.
    let other = a2m_core::Digest::of(b"another diff");
    let stale = store
        .write(Durability::Synced, move |tx| {
            let approval = record_approval(tx, fence.run, &other)?;
            let clearance = consume(tx, approval)?;
            tx.ledger().begin_writes(&fence, clearance)
        })
        .await
        .unwrap_err();
    assert_eq!(
        (stale.class(), stale.code()),
        (FailureClass::Policy, &codes::APPROVAL_STALE)
    );

    // Not with an approval nobody marked used.
    let unused = store
        .write(Durability::Synced, move |tx| {
            let approval = record_approval(tx, fence.run, &diff_hash())?;
            tx.ledger()
                .begin_writes(&fence, WriteClearance::Approval(approval))
        })
        .await
        .unwrap_err();
    assert_eq!(unused.code(), &codes::APPROVAL_MISSING);

    // Not with an approval marked used in an earlier transaction: marking it
    // used and starting the writes are one transaction or nothing.
    let earlier = store
        .write(Durability::Synced, move |tx| {
            let approval = record_approval(tx, fence.run, &diff_hash())?;
            consume(tx, approval)
        })
        .await
        .unwrap();
    let split = store
        .write(Durability::Synced, move |tx| {
            tx.ledger().begin_writes(&fence, earlier)
        })
        .await
        .unwrap_err();
    assert_eq!(split.code(), &codes::APPROVAL_MISSING);

    // Not with an approval for another run, nor a decision that said no.
    let elsewhere = previewed(&store, 2, &["z"]).await;
    let borrowed = store
        .write(Durability::Synced, move |tx| {
            let approval = record_approval(tx, elsewhere.run, &diff_hash())?;
            let clearance = consume(tx, approval)?;
            tx.ledger().begin_writes(&fence, clearance)
        })
        .await
        .unwrap_err();
    assert_eq!(borrowed.code(), &codes::APPROVAL_MISSING);
    let denied = store
        .write(Durability::Synced, move |tx| {
            let decision = record_decision(tx, fence.run, &diff_hash(), false)?;
            tx.ledger()
                .begin_writes(&fence, WriteClearance::PreApproved(decision))
        })
        .await
        .unwrap_err();
    assert_eq!(denied.code(), &codes::APPROVAL_MISSING);

    // Every refusal took its whole transaction back: the run still waits.
    let run = store
        .read(move |tx| tx.ledger().run(fence.run))
        .await
        .unwrap();
    assert_eq!(run.state, RunState::NeedsApproval);
    assert!(!run.cleared_to_write());

    // With an approval of this diff, marked used in the same transaction.
    let approval = store
        .write(Durability::Synced, move |tx| {
            let approval = record_approval(tx, fence.run, &diff_hash())?;
            let clearance = consume(tx, approval)?;
            tx.ledger().begin_writes(&fence, clearance)?;
            Ok(approval)
        })
        .await
        .unwrap();
    let run = store
        .read(move |tx| tx.ledger().run(fence.run))
        .await
        .unwrap();
    assert_eq!(run.state, RunState::Running);
    assert_eq!(run.approval, Some(approval));

    // Or with a recorded decision that this run may write without a person.
    let fence = previewed(&store, 3, &["b"]).await;
    store
        .write(Durability::Synced, move |tx| {
            let decision = record_decision(tx, fence.run, &diff_hash(), true)?;
            tx.ledger()
                .begin_writes(&fence, WriteClearance::PreApproved(decision))
        })
        .await
        .unwrap();
    let run = store
        .read(move |tx| tx.ledger().run(fence.run))
        .await
        .unwrap();
    assert_eq!(run.state, RunState::Running);
    assert!(run.decision.is_some());
}

/// Rule 4. One writer per run.
#[tokio::test]
async fn rule_4_only_the_current_lease_may_change_a_run() {
    let store = memory().await;
    let old = running(&store, 1, &["a", "b"]).await;
    lock(&store, old, 0).await.unwrap();
    let new = store
        .write(Durability::Synced, move |tx| tx.ledger().new_epoch(old.run))
        .await
        .unwrap();
    assert_eq!(new.epoch, Epoch::new(old.epoch.get() + 1));

    let lost = |failure: a2m_core::Failure| {
        assert_eq!(failure.class(), FailureClass::PossibleCommit);
        assert_eq!(failure.code(), &codes::RUNNER_LOST);
    };
    lost(lock(&store, old, 1).await.unwrap_err());
    lost(intent(&store, old, 0, "a").await.unwrap_err());
    lost(
        store
            .write(Durability::Synced, move |tx| {
                tx.ledger().set_state(&old, RunState::Cancelled, None)
            })
            .await
            .unwrap_err(),
    );
    lost(
        store
            .write(Durability::Synced, move |tx| {
                let why = stopped_by(FailureClass::Transient, "timeout");
                tx.ledger().mark_failed(&old, record(&old, 0), &why, None)
            })
            .await
            .unwrap_err(),
    );
    lost(
        store
            .write(Durability::Synced, move |tx| {
                tx.ledger()
                    .settle(&old, record(&old, 0), Reconciled::NotLanded)
            })
            .await
            .unwrap_err(),
    );

    // The new lease works, and a lease number from nowhere does not.
    lock(&store, new, 0).await.unwrap();
    let forged = Fence {
        run: old.run,
        epoch: Epoch::new(99),
    };
    lost(lock(&store, forged, 0).await.unwrap_err());
}

/// Rule 5. Reconcile after any doubt.
#[tokio::test]
async fn rule_5_nothing_is_written_while_a_record_is_in_doubt_from_an_earlier_lease() {
    let store = memory().await;
    let old = running(&store, 1, &["a", "b"]).await;
    lock(&store, old, 0).await.unwrap();
    // The intent is stored, and then the runner is gone: the write may or
    // may not have been sent.
    let _gone = intent(&store, old, 0, "a").await.unwrap();
    let new = store
        .write(Durability::Synced, move |tx| tx.ledger().new_epoch(old.run))
        .await
        .unwrap();

    // The reconcile query finds it, and its lock is quarantined, not released.
    let doubtful = store
        .read(move |tx| tx.ledger().in_doubt(new.run))
        .await
        .unwrap();
    assert_eq!(doubtful.len(), 1);
    assert_eq!(doubtful[0].record, record(&new, 0));
    assert_eq!(doubtful[0].state, RecordState::Intent);
    let locks = store
        .read(move |tx| tx.ledger().locks(new.run))
        .await
        .unwrap();
    assert!(locks.iter().all(|lock| lock.quarantined) && locks.len() == 1);

    // No new intent, for any record of the run, until it has been read back.
    lock(&store, new, 1).await.unwrap();
    let refused = intent(&store, new, 1, "b").await.unwrap_err();
    assert_eq!(refused.class(), FailureClass::PossibleCommit);
    assert_eq!(refused.code(), &codes::RECONCILE_REQUIRED);
    assert_eq!(state_of(&store, new, 1).await, RecordState::Planned);

    // Settling asks for the lock under the new lease first.
    let unlocked = store
        .write(Durability::Synced, move |tx| {
            tx.ledger()
                .settle(&new, record(&new, 0), Reconciled::NotLanded)
        })
        .await
        .unwrap_err();
    assert_eq!(unlocked.code(), &codes::LEDGER_STATE_UNEXPECTED);
    lock(&store, new, 0).await.unwrap();
    let settled = store
        .write(Durability::Synced, move |tx| {
            tx.ledger()
                .settle(&new, record(&new, 0), Reconciled::NotLanded)
        })
        .await
        .unwrap();
    assert_eq!(settled, Settled::Planned);

    // Now the run writes again, the settled record included.
    let _second = intent(&store, new, 1, "b").await.unwrap();
    let _first = intent(&store, new, 0, "a").await.unwrap();
}

/// Rule 6. One run per identical call.
#[tokio::test]
async fn rule_6_a_second_identical_call_gets_the_first_run() {
    let store = memory().await;
    let open = |caller: u8, input: &'static str| {
        let store = store.clone();
        async move {
            store
                .write(Durability::Synced, move |tx| {
                    tx.ledger()
                        .open_run(new_run(EffectClass::Read, caller, input))
                })
                .await
                .unwrap()
        }
    };
    let Opened::New(first) = open(1, "same").await else {
        panic!("the first call opens a run");
    };
    assert_eq!(open(1, "same").await, Opened::Existing(first.run));
    // Another caller, or other inputs, is another run.
    assert!(matches!(open(2, "same").await, Opened::New(_)));
    assert!(matches!(open(1, "other").await, Opened::New(_)));

    // Once the first is finished, the same call starts a new run.
    store
        .write(Durability::Synced, move |tx| {
            tx.ledger().set_state(&first, RunState::Cancelled, None)
        })
        .await
        .unwrap();
    let Opened::New(second) = open(1, "same").await else {
        panic!("a finished run is not handed back");
    };
    assert_ne!(second.run, first.run);
}

/// Rule 7. One lock per record.
#[tokio::test]
async fn rule_7_two_runs_cannot_both_hold_the_lock_on_one_record() {
    let folder = Folder::new("rule7");
    let store = on_disk(&folder).await;
    let one = running(&store, 1, &["shared"]).await;
    let two = running(&store, 2, &["shared"]).await;

    // Both ask at once; one is refused.
    let (first, second) = tokio::join!(lock(&store, one, 0), lock(&store, two, 0));
    let (holder, waiter, refused) = match (first, second) {
        (Ok(()), Err(refused)) => (one, two, refused),
        (Err(refused), Ok(())) => (two, one, refused),
        other => panic!("exactly one run takes the lock: {other:?}"),
    };
    assert_eq!(refused.class(), FailureClass::Busy);
    assert_eq!(refused.code(), &codes::RECORD_LOCKED);

    // A second connection cannot add a second lock on the record either.
    let second = folder
        .raw()
        .execute(
            "INSERT INTO record_locks
             SELECT workspace_id, target, environment, record_key_hash, ?1, 1, 'held', 0
             FROM record_locks",
            [waiter.run.to_string()],
        )
        .unwrap_err();
    assert!(second.to_string().contains("UNIQUE"), "{second}");

    // Without the lock, no intent.
    let no_intent = intent(&store, waiter, 0, "shared").await.unwrap_err();
    assert_eq!(no_intent.code(), &codes::RECORD_LOCKED);

    // The holder stops with the record in doubt: the lock is quarantined,
    // and the other run still cannot take it.
    let _sent = intent(&store, holder, 0, "shared").await.unwrap();
    store
        .write(Durability::Synced, move |tx| {
            let why = stopped_by(FailureClass::PossibleCommit, "no_confirmation");
            tx.ledger()
                .set_state(&holder, RunState::Stopped, Some(&why))
        })
        .await
        .unwrap();
    let locks = store
        .read(move |tx| tx.ledger().locks(holder.run))
        .await
        .unwrap();
    assert!(locks.len() == 1 && locks[0].quarantined);
    assert_eq!(
        lock(&store, waiter, 0).await.unwrap_err().code(),
        &codes::RECORD_LOCKED
    );

    // Once the holder's record has been read back and settled, the lock is
    // free and the other run takes it.
    lock(&store, holder, 0).await.unwrap();
    let proof = evidence(&store, "read").await;
    store
        .write(Durability::Synced, move |tx| {
            let read = ReadBack {
                digest: intended("shared"),
                after: None,
                evidence: proof,
            };
            tx.ledger()
                .settle(&holder, record(&holder, 0), Reconciled::Landed(read))
        })
        .await
        .unwrap();
    lock(&store, waiter, 0).await.unwrap();
}

/// Rule 8. Final means final.
#[tokio::test]
async fn rule_8_a_run_in_a_final_state_never_changes() {
    let folder = Folder::new("rule8");
    let store = on_disk(&folder).await;
    let fence = running(&store, 1, &["a"]).await;
    store
        .write(Durability::Synced, move |tx| {
            tx.ledger().set_state(&fence, RunState::Cancelled, None)
        })
        .await
        .unwrap();

    for to in RunState::ALL {
        let to = *to;
        let refused = store
            .write(Durability::Synced, move |tx| {
                let why = stopped_by(FailureClass::Transient, "timeout");
                tx.ledger().set_state(&fence, to, Some(&why))
            })
            .await
            .unwrap_err();
        assert_eq!(refused.code(), &codes::LEDGER_STATE_UNEXPECTED, "{to}");
    }
    let refused = store
        .write(Durability::Synced, move |tx| {
            tx.ledger().new_epoch(fence.run)
        })
        .await
        .unwrap_err();
    assert_eq!(refused.code(), &codes::LEDGER_STATE_UNEXPECTED);
    assert_eq!(
        lock(&store, fence, 0).await.unwrap_err().code(),
        &codes::LEDGER_STATE_UNEXPECTED
    );

    // The store refuses the same things from anyone.
    let raw = folder.raw();
    for statement in [
        "UPDATE runs SET state = 'running'",
        "UPDATE runs SET lease_epoch = lease_epoch + 1",
        "UPDATE run_records SET state = 'intent', intent_epoch = 1, intent_seq = 1",
        "INSERT INTO run_events (run_id, seq, epoch, at, kind, payload)
         SELECT id, 99, lease_epoch, 0, 'state_changed', '{}' FROM runs",
        "DELETE FROM runs",
    ] {
        assert!(raw.execute(statement, []).is_err(), "{statement}");
    }
    let state: String = raw
        .query_row("SELECT state FROM runs", [], |row| row.get(0))
        .unwrap();
    assert_eq!(state, "cancelled");
}

/// Rule 9. History is append-only.
#[tokio::test]
async fn rule_9_events_are_never_updated_or_deleted() {
    let folder = Folder::new("rule9");
    let store = on_disk(&folder).await;
    let fence = running(&store, 1, &["a"]).await;
    let before = store
        .read(move |tx| tx.ledger().events(fence.run, 0, 1000))
        .await
        .unwrap();
    assert!(before.len() > 5);

    let raw = folder.raw();
    for statement in [
        "UPDATE run_events SET payload = '{}'",
        "UPDATE run_events SET seq = seq + 100",
        "DELETE FROM run_events",
        "DELETE FROM run_events WHERE seq = 2",
        // An event out of order, or from a lease that is not the current one.
        "INSERT INTO run_events (run_id, seq, epoch, at, kind, payload)
         SELECT id, 1000, lease_epoch, 0, 'need', '{}' FROM runs",
        "INSERT INTO run_events (run_id, seq, epoch, at, kind, payload)
         SELECT id, (SELECT MAX(seq) + 1 FROM run_events), 0, 0, 'need', '{}' FROM runs",
    ] {
        let refused = raw.execute(statement, []).unwrap_err();
        assert!(
            refused.to_string().contains("a2m: "),
            "{statement}: {refused}"
        );
    }
    let after = store
        .read(move |tx| tx.ledger().events(fence.run, 0, 1000))
        .await
        .unwrap();
    assert_eq!(after, before);

    // The ledger's own kinds cannot be written through the engine's call.
    let forged = store
        .write(Durability::Synced, move |tx| {
            tx.ledger().append_event(
                &fence,
                a2m_ledger::EventKind::Verified,
                &serde_json::json!({ "step": 0, "record": 0 }),
                None,
            )
        })
        .await
        .unwrap_err();
    assert_eq!(forged.code(), &codes::LEDGER_STATE_UNEXPECTED);
}

/// The intent token is the write gate, so it must be impossible to make one,
/// copy one, or conjure one from nothing outside the ledger. Building one by
/// hand and cloning one are compile failures, shown in the type's own
/// documentation tests. This holds the rest: each line compiles only while
/// the token does not have the trait named.
#[test]
fn an_intent_token_cannot_be_copied_or_made_from_nothing() {
    trait OnlyWithout<Marker> {
        fn holds() {}
    }
    impl<T: ?Sized> OnlyWithout<()> for T {}
    struct IsClone;
    impl<T: Clone> OnlyWithout<IsClone> for T {}
    struct IsDefault;
    impl<T: Default> OnlyWithout<IsDefault> for T {}

    // If `IntentToken` were `Clone` or `Default`, the marker could not be
    // inferred and these lines would not compile.
    <a2m_ledger::IntentToken as OnlyWithout<_>>::holds();
    <a2m_ledger::ActedToken as OnlyWithout<_>>::holds();
}
