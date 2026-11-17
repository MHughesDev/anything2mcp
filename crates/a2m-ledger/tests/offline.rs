//! The clearance of an offline test (decision 0180): taken by the store an
//! offline test opens for itself, and by no other store.

#![allow(
    clippy::unwrap_used,
    reason = "this file holds only tests; clippy.toml allows unwrap in tests, which it tells by the #[test] attribute, and the helpers below do not carry it"
)]

mod common;

use std::sync::Arc;

use a2m_core::{RunState, Timestamp};
use a2m_ledger::{Durability, Store, StoreConfig, WriteClearance, codes};
use a2m_telemetry::ManualClock;
use a2m_vault::Vault;
use common::*;

async fn throwaway() -> Store {
    let store = Store::open(
        StoreConfig::throwaway()
            .with_clock(Arc::new(ManualClock::starting_at(
                Timestamp::from_unix_millis(1_700_000_000_000),
            )))
            .with_random(Counted::new()),
    )
    .await
    .unwrap();
    let vault = Arc::new(Vault::with_random(
        key_store(),
        Arc::new(store.key_records()),
        Counted::new(),
    ));
    store.attach(vault).await.unwrap();
    store
}

#[tokio::test]
async fn only_a_throwaway_store_takes_an_offline_tests_clearance() {
    // The installation's kind of store, in a folder or in memory, refuses
    // it, and the run stays waiting for a person.
    let folder = Folder::new("offline-clearance");
    for store in [memory().await, on_disk(&folder).await] {
        assert!(!store.is_throwaway());
        let fence = previewed(&store, 1, &["a"]).await;
        let refused = store
            .write(Durability::Synced, move |tx| {
                tx.ledger().begin_writes(&fence, WriteClearance::OfflineTest)
            })
            .await
            .unwrap_err();
        assert_eq!(refused.code(), &codes::APPROVAL_MISSING);
        let run = store
            .read(move |tx| tx.ledger().run(fence.run))
            .await
            .unwrap();
        assert_eq!(run.state, RunState::NeedsApproval);
        assert!(!run.cleared_to_write());
    }

    // The offline test's own store takes it once: the run is running and
    // reads as cleared, and the rules of every other store still hold.
    let store = throwaway().await;
    assert!(store.is_throwaway());
    let fence = previewed(&store, 1, &["a"]).await;
    store
        .write(Durability::Synced, move |tx| {
            tx.ledger().begin_writes(&fence, WriteClearance::OfflineTest)
        })
        .await
        .unwrap();
    let run = store
        .read(move |tx| tx.ledger().run(fence.run))
        .await
        .unwrap();
    assert_eq!(run.state, RunState::Running);
    assert!(run.cleared_to_write());
    let again = store
        .write(Durability::Synced, move |tx| {
            tx.ledger().begin_writes(&fence, WriteClearance::OfflineTest)
        })
        .await
        .unwrap_err();
    assert_eq!(again.code(), &codes::LEDGER_STATE_UNEXPECTED);
    // Intent before action holds here as anywhere.
    lock(&store, fence, 0).await.unwrap();
    let token = intent(&store, fence, 0, "a").await.unwrap();
    assert_eq!(token.record(), record(&fence, 0));
}
