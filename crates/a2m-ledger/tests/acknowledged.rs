//! A record that ends on the system's acknowledgement (decisions 0161 and
//! 0172), and a failure that keeps what the system said. Each acceptance
//! and each refusal, through the calls and, where a statement can break the
//! rule, through a second connection to the database file.

#![allow(
    clippy::unwrap_used,
    reason = "this file holds only tests; clippy.toml allows unwrap in tests, which it tells by the #[test] attribute, and the helpers below do not carry it"
)]

mod common;

use a2m_core::{EffectClass, FailureClass, RecordState, RunState};
use a2m_ledger::{
    Acted, ActedToken, BlobKind, BlobRef, Durability, Fence, NewBlob, Opened, ReadBack, Reconciled,
    RetentionClass, Settled, Store, codes, replay,
};
use common::*;

/// A diff as the engine stores it, by the names the ledger reads: record 0's
/// write ends on an acknowledgement, the others are read back.
fn diff_text(records: u32) -> String {
    let entries: Vec<String> = (0..records)
        .map(|at| {
            let evidence = if at == 0 {
                r#","evidence":"acknowledged""#
            } else {
                ""
            };
            format!(
                r#"{{"step_seq":0,"record_seq":{at},"operation":"example_order_update","action":"update","fields":[]{evidence}}}"#
            )
        })
        .collect();
    format!(
        r#"{{"tool":"example_order_update","records":[{}]}}"#,
        entries.join(",")
    )
}

async fn diff_blob(store: &Store, records: u32) -> BlobRef {
    store
        .blobs()
        .put(NewBlob {
            workspace: workspace(),
            owner: a2m_vault::OwnerRef::service("engine").unwrap(),
            kind: BlobKind::Diff,
            class: RetentionClass::Long,
            plain: diff_text(records).into_bytes(),
        })
        .await
        .unwrap()
}

/// A run of `effect` with the named records planned, its diff stored as a
/// blob, approved, and `running`.
async fn cleared(store: &Store, effect: EffectClass, names: &[&str]) -> Fence {
    let blob = diff_blob(store, u32::try_from(names.len()).unwrap()).await;
    let names: Vec<String> = names.iter().map(|name| (*name).to_owned()).collect();
    store
        .write(Durability::Synced, move |tx| {
            let mut ledger = tx.ledger();
            let Opened::New(opened) = ledger.open_run(new_run(effect, 1, &names.join(",")))? else {
                panic!("the run exists already");
            };
            let fence = ledger.new_epoch(opened.run)?;
            ledger.set_state(&fence, RunState::Validating, None)?;
            let records: Vec<_> = names
                .iter()
                .enumerate()
                .map(|(at, name)| planned(u32::try_from(at).unwrap(), name))
                .collect();
            ledger.plan_records(&fence, &records)?;
            ledger.store_diff(&fence, diff_hash(), Some(blob))?;
            ledger.set_state(&fence, RunState::NeedsApproval, None)?;
            let approval = record_approval(tx, fence.run, &diff_hash())?;
            let clearance = consume(tx, approval)?;
            tx.ledger().begin_writes(&fence, clearance)?;
            Ok(fence)
        })
        .await
        .unwrap()
}

/// Locks, intends, and marks one record `acted`.
async fn sent(store: &Store, fence: Fence, at: u32, name: &str) -> ActedToken {
    lock(store, fence, at).await.unwrap();
    let token = intent(store, fence, at, name).await.unwrap();
    store
        .write(Durability::Synced, move |tx| {
            tx.ledger().mark_acted(token, Acted::default())
        })
        .await
        .unwrap()
}

async fn row(store: &Store, fence: Fence, at: u32) -> a2m_ledger::RecordRow {
    store
        .read(move |tx| tx.ledger().record(record(&fence, at)))
        .await
        .unwrap()
}

#[tokio::test]
async fn a_record_ends_on_an_acknowledgement_and_its_run_completes() {
    let folder = Folder::new("acknowledged");
    let store = on_disk(&folder).await;
    let fence = cleared(&store, EffectClass::Irreversible, &["a", "b"]).await;
    let acked = sent(&store, fence, 0, "a").await;
    let answer = evidence(&store, "what the save answered").await;
    store.store_acknowledged(&acked, answer).await.unwrap();

    // It stays `acted`, is never called verified, and is settled: not in
    // doubt, its lock released, its evidence kept.
    let ended = row(&store, fence, 0).await;
    assert_eq!(ended.state, RecordState::Acted);
    assert!(ended.acknowledged() && !ended.in_doubt());
    assert_eq!(
        ended.ended_on(),
        Some(a2m_core::WriteEvidence::Acknowledged)
    );
    assert_eq!(ended.evidence, Some(answer));
    let raw = folder.raw();
    let read_back: Option<String> = raw
        .query_row(
            "SELECT read_back_hash FROM run_records WHERE record_seq = 0",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        read_back, None,
        "an acknowledged record never counts as read back"
    );
    let locks = store
        .read(move |tx| tx.ledger().locks(fence.run))
        .await
        .unwrap();
    assert!(locks.is_empty());

    // The run does not complete while another record is open.
    let verify_later = sent(&store, fence, 1, "b").await;
    let early = store
        .write(Durability::Synced, move |tx| {
            tx.ledger().set_state(&fence, RunState::Completed, None)
        })
        .await
        .unwrap_err();
    assert_eq!(early.code(), &codes::LEDGER_STATE_UNEXPECTED);
    let proof = evidence(&store, "what was read").await;
    store
        .store_verified(
            &verify_later,
            ReadBack {
                digest: intended("b"),
                after: None,
                evidence: proof,
            },
        )
        .await
        .unwrap();

    // Verified and acknowledged: it completes, and says how many were not
    // read back.
    let tally = store
        .read(move |tx| tx.ledger().tally(fence.run))
        .await
        .unwrap();
    assert_eq!((tally.verified, tally.acknowledged, tally.open), (1, 1, 0));
    store
        .write(Durability::Synced, move |tx| {
            tx.ledger().set_state(&fence, RunState::Completed, None)
        })
        .await
        .unwrap();

    // The events alone give the same state.
    let events = store
        .read(move |tx| tx.ledger().events(fence.run, 0, 1000))
        .await
        .unwrap();
    let rebuilt = replay(&events).unwrap();
    assert_eq!(rebuilt.state, RunState::Completed);
    assert_eq!(rebuilt.records.get(&(0, 0)), Some(&RecordState::Acted));
    assert_eq!(rebuilt.records.get(&(0, 1)), Some(&RecordState::Verified));
    assert!(rebuilt.acknowledged.contains(&(0, 0)) && rebuilt.acknowledged.len() == 1);
}

#[tokio::test]
async fn an_acknowledgement_is_refused_where_the_diff_or_the_record_does_not_allow_it() {
    let folder = Folder::new("acknowledged-refused");
    let store = on_disk(&folder).await;
    let fence = cleared(&store, EffectClass::Irreversible, &["a", "b"]).await;
    let answer = evidence(&store, "what the save answered").await;

    // The diff says record 1 is read back: the caller's word does not end it.
    let read_back = sent(&store, fence, 1, "b").await;
    let refused = store
        .store_acknowledged(&read_back, answer)
        .await
        .unwrap_err();
    assert_eq!(refused.code(), &codes::LEDGER_STATE_UNEXPECTED);
    assert!(refused.message().contains("must be read back"), "{refused}");
    assert!(row(&store, fence, 1).await.in_doubt());

    // A record with no intent: nothing marks it, and the store refuses it
    // whoever sends the statement.
    let raw = folder.raw();
    let early = raw
        .execute(
            "UPDATE run_records SET acknowledged_at = 1, evidence_ref = ?1 WHERE record_seq = 0",
            [answer.to_string()],
        )
        .unwrap_err();
    assert!(early.to_string().contains("only from acted"), "{early}");

    // Evidence that names no blob is refused.
    let acked = sent(&store, fence, 0, "a").await;
    let nothing = BlobRef::from_digest(a2m_core::Digest::of(b"no such blob"));
    let missing = store.store_acknowledged(&acked, nothing).await.unwrap_err();
    assert_eq!(missing.code(), &codes::LEDGER_STATE_UNEXPECTED);
    assert!(row(&store, fence, 0).await.in_doubt());

    // Accepted once, then settled: a second time, a read-back, and a
    // failure are refused, and the store refuses any change to it.
    store.store_acknowledged(&acked, answer).await.unwrap();
    let twice = store.store_acknowledged(&acked, answer).await.unwrap_err();
    assert_eq!(twice.code(), &codes::LEDGER_STATE_UNEXPECTED);
    assert!(twice.message().contains("before"), "{twice}");
    let verified = store
        .store_verified(
            &acked,
            ReadBack {
                digest: intended("a"),
                after: None,
                evidence: answer,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(verified.code(), &codes::LEDGER_STATE_UNEXPECTED);
    assert!(verified.message().contains("never verified"), "{verified}");
    let failed = store
        .write(Durability::Synced, move |tx| {
            tx.ledger().mark_failed(
                &fence,
                record(&fence, 0),
                &stopped_by(FailureClass::Busy, "store_unavailable"),
                None,
            )
        })
        .await
        .unwrap_err();
    assert_eq!(failed.code(), &codes::LEDGER_STATE_UNEXPECTED);
    let changed = raw
        .execute(
            "UPDATE run_records SET state = 'failed', failure_class = 'busy' WHERE record_seq = 0",
            [],
        )
        .unwrap_err();
    assert!(changed.to_string().contains("never changes"), "{changed}");
    assert!(row(&store, fence, 0).await.acknowledged());

    // A token of a lease that ended changes nothing.
    let other = cleared(&store, EffectClass::Irreversible, &["c"]).await;
    let lost = sent(&store, other, 0, "c").await;
    store
        .write(Durability::Synced, move |tx| {
            tx.ledger().new_epoch(other.run)
        })
        .await
        .unwrap();
    let late = store.store_acknowledged(&lost, answer).await.unwrap_err();
    assert_eq!(late.code(), &codes::RUNNER_LOST);
}

#[tokio::test]
async fn only_an_irreversible_run_ends_a_record_on_an_acknowledgement() {
    let store = memory().await;
    // Even where its diff says so, a run that is not irreversible cannot.
    let fence = cleared(&store, EffectClass::Write, &["a"]).await;
    let acked = sent(&store, fence, 0, "a").await;
    let answer = evidence(&store, "what the save answered").await;
    let refused = store.store_acknowledged(&acked, answer).await.unwrap_err();
    assert_eq!(refused.code(), &codes::LEDGER_STATE_UNEXPECTED);
    assert!(refused.message().contains("irreversible"), "{refused}");
    assert!(row(&store, fence, 0).await.in_doubt());
}

/// Rule 5 on a resume: a record that ended on an acknowledgement under an
/// earlier lease is settled, not in doubt, and does not stop the next
/// lease's intents.
#[tokio::test]
async fn on_a_resume_an_acknowledged_record_is_settled() {
    let store = memory().await;
    let old = cleared(&store, EffectClass::Irreversible, &["a", "b", "c"]).await;
    let acked = sent(&store, old, 0, "a").await;
    let answer = evidence(&store, "what the save answered").await;
    store.store_acknowledged(&acked, answer).await.unwrap();
    let _gone = sent(&store, old, 1, "b").await;
    let new = store
        .write(Durability::Synced, move |tx| tx.ledger().new_epoch(old.run))
        .await
        .unwrap();

    let doubtful = store
        .read(move |tx| tx.ledger().in_doubt(new.run))
        .await
        .unwrap();
    assert_eq!(doubtful.len(), 1);
    assert_eq!(doubtful[0].record, record(&new, 1));
    let locks = store
        .read(move |tx| tx.ledger().locks(new.run))
        .await
        .unwrap();
    assert!(locks.len() == 1 && locks[0].quarantined);
    let resumed = row(&store, new, 0).await;
    assert!(resumed.acknowledged() && !resumed.in_doubt());

    // A settled record needs no lock and no read.
    let no_lock = lock(&store, new, 0).await.unwrap_err();
    assert_eq!(no_lock.code(), &codes::LEDGER_STATE_UNEXPECTED);
    let no_read = store
        .write(Durability::Synced, move |tx| {
            tx.ledger().settle(
                &new,
                record(&new, 0),
                Reconciled::NotLanded { evidence: answer },
            )
        })
        .await
        .unwrap_err();
    assert_eq!(no_read.code(), &codes::LEDGER_STATE_UNEXPECTED);

    // Once the record in doubt is read, the run writes again: the
    // acknowledged record of the earlier lease does not hold it back.
    lock(&store, new, 1).await.unwrap();
    let settled = store
        .write(Durability::Synced, move |tx| {
            tx.ledger().settle(
                &new,
                record(&new, 1),
                Reconciled::NotLanded { evidence: answer },
            )
        })
        .await
        .unwrap();
    assert_eq!(settled, Settled::Planned);
    lock(&store, new, 2).await.unwrap();
    let _next = intent(&store, new, 2, "c").await.unwrap();
}

/// A failure keeps what the system said, apart from the sentence, where the
/// ledger stores a run's failure and a record's, and hands it back so.
#[tokio::test]
async fn a_stored_failure_keeps_what_the_system_said_apart() {
    let store = memory().await;
    let fence = running(&store, 1, &["a"]).await;
    lock(&store, fence, 0).await.unwrap();
    let _token = intent(&store, fence, 0, "a").await.unwrap();
    let why = stopped_by(FailureClass::Mismatch, "read_back_differs")
        .with_said("field `total` must be at most 10\u{7}");
    let stored = why.clone();
    store
        .write(Durability::Synced, move |tx| {
            let mut ledger = tx.ledger();
            ledger.mark_failed(&fence, record(&fence, 0), &stored, None)?;
            ledger.set_state(&fence, RunState::Stopped, Some(&stored))
        })
        .await
        .unwrap();

    let (run_failure, record_failure) = store
        .read(move |tx| {
            let mut ledger = tx.ledger();
            Ok((
                ledger.run_failure(fence.run)?,
                ledger.record_failure(record(&fence, 0))?,
            ))
        })
        .await
        .unwrap();
    for read in [run_failure.unwrap(), record_failure.unwrap()] {
        assert_eq!(read, why);
        // Stored and handed back raw, a control character too: a door
        // escapes it once where it shows it (decision 0193).
        assert_eq!(read.said(), Some("field `total` must be at most 10\u{7}"));
        assert_eq!(read.message(), "It stopped.");
        assert_eq!(read.next_step(), "Look.");
    }

    // A run with no failure gives none.
    let other = running(&store, 2, &["b"]).await;
    let none = store
        .read(move |tx| tx.ledger().run_failure(other.run))
        .await
        .unwrap();
    assert!(none.is_none());
}

/// What a record recorded before its save is held by it only while its
/// write landed or may have landed (decision 0196): not while it is
/// planned, and not once it failed unsent and settled.
#[tokio::test]
async fn a_record_holds_what_it_recorded_only_while_its_write_may_have_landed() {
    let store = memory().await;
    let fence = running(&store, 1, &["a", "b", "c"]).await;
    lock(&store, fence, 0).await.unwrap();
    let _token = intent(&store, fence, 0, "a").await.unwrap();
    lock(&store, fence, 2).await.unwrap();
    let _other = intent(&store, fence, 2, "c").await.unwrap();
    let refused = stopped_by(FailureClass::Input, "input_invalid");
    store
        .write(Durability::Synced, move |tx| {
            tx.ledger()
                .mark_failed(&fence, record(&fence, 2), &refused, None)
        })
        .await
        .unwrap();
    let rows = store
        .read(move |tx| tx.ledger().records(fence.run))
        .await
        .unwrap();
    let landed: Vec<bool> = rows.iter().map(a2m_ledger::RecordRow::may_have_landed).collect();
    // In doubt with its intent; planned; failed with nothing sent, settled.
    assert_eq!(landed, [true, false, false], "{rows:?}");
}
