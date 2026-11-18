//! A binding in shadow: its comparisons are counted by its digest, a
//! mismatch starts the run of matches again, and a person's promotion is
//! recorded only on a run of exact matches at the threshold (decision 0207).

#![allow(
    clippy::unwrap_used,
    reason = "this file holds only tests; clippy.toml allows unwrap in tests, which it tells by the #[test] attribute, and the helpers the tests share do not carry it"
)]

mod common;

use std::sync::Arc;

use a2m_core::{
    Digest, EffectClass, FailureClass, FailureCode, Mode, OperationName, RunState, Timestamp,
};
use a2m_ledger::{
    Difference, DifferingOutput, Durability, Fence, NotPromotable, Opened, Promotable,
    ShadowComparison, ShadowOutcome, ShadowPromotion, ShadowSubject, ShadowThreshold, Store,
    StoreConfig, codes,
};
use a2m_telemetry::ManualClock;
use a2m_vault::Vault;
use common::*;

fn operation() -> OperationName {
    OperationName::new("example_order_get").unwrap()
}

fn shadow_digest() -> Digest {
    Digest::of(b"the shadow binding")
}

fn subject(binding: Digest, writes: bool) -> ShadowSubject {
    ShadowSubject {
        workspace: workspace(),
        target: target(),
        operation: operation(),
        binding,
        writes,
    }
}

/// A read run of its own, `running`, as the engine moves one.
async fn reading(store: &Store, input: &str) -> Fence {
    let input = input.to_owned();
    store
        .write(Durability::Synced, move |tx| {
            let mut ledger = tx.ledger();
            let Opened::New(opened) = ledger.open_run(new_run(EffectClass::Read, 1, &input))?
            else {
                panic!("the run exists already");
            };
            let fence = ledger.new_epoch(opened.run)?;
            ledger.set_state(&fence, RunState::Validating, None)?;
            ledger.set_state(&fence, RunState::Running, None)?;
            Ok(fence)
        })
        .await
        .unwrap()
}

fn compared(fence: Fence, binding: Digest, outcome: ShadowOutcome) -> ShadowComparison {
    ShadowComparison {
        fence,
        operation: operation(),
        binding,
        mode: Mode::new("app_json_spare").unwrap(),
        position: 1,
        against: Digest::of(b"the verified binding"),
        writes: false,
        outcome,
    }
}

async fn keep(store: &Store, comparison: ShadowComparison) -> Result<u64, a2m_core::Failure> {
    store
        .write(Durability::Synced, move |tx| {
            tx.shadow().record(&comparison)
        })
        .await
}

async fn promotable(store: &Store, binding: Digest, writes: bool) -> Promotable {
    store
        .read(move |tx| {
            tx.shadow()
                .promotable(&subject(binding, writes), ShadowThreshold::default())
        })
        .await
        .unwrap()
}

async fn promote(store: &Store, binding: Digest) -> Result<a2m_ledger::PromotionRow, a2m_core::Failure> {
    store
        .write(Durability::Synced, move |tx| {
            tx.shadow().promote(&ShadowPromotion {
                subject: subject(binding, false),
                mode: Mode::new("app_json_spare").unwrap(),
                promoted_by: principal(9),
                threshold: ShadowThreshold::default(),
            })
        })
        .await
}

fn differs(name: &str) -> ShadowOutcome {
    ShadowOutcome::Mismatched(vec![DifferingOutput {
        name: name.to_owned(),
        how: Difference::Differs,
    }])
}

#[tokio::test]
async fn a_binding_is_promoted_only_after_the_set_number_of_exact_matches_in_a_row() {
    let store = memory().await;
    let fence = reading(&store, "a").await;
    let binding = shadow_digest();

    // Nothing compared: not promotable, and the report is empty.
    assert_eq!(
        promotable(&store, binding, false).await,
        Promotable::No(NotPromotable::NothingCompared)
    );

    // Four matches, one not compared: too few, and the one not compared
    // neither counts nor starts the count again.
    for _ in 0..4 {
        keep(&store, compared(fence, binding, ShadowOutcome::Matched))
            .await
            .unwrap();
    }
    keep(
        &store,
        compared(
            fence,
            binding,
            ShadowOutcome::NotCompared {
                class: FailureClass::Busy,
                code: FailureCode::fixed("shadow_time_exceeded"),
            },
        ),
    )
    .await
    .unwrap();
    assert_eq!(
        promotable(&store, binding, false).await,
        Promotable::No(NotPromotable::TooFew {
            in_a_row: 4,
            threshold: 5
        })
    );
    let refused = promote(&store, binding).await.unwrap_err();
    assert_eq!(refused.code(), &codes::SHADOW_NOT_PROMOTABLE);
    assert_eq!(refused.class(), FailureClass::Policy);

    // A mismatch starts the run again: four more matches are still too few.
    keep(&store, compared(fence, binding, differs("customer")))
        .await
        .unwrap();
    for _ in 0..4 {
        keep(&store, compared(fence, binding, ShadowOutcome::Matched))
            .await
            .unwrap();
    }
    let Promotable::No(NotPromotable::MismatchSince {
        in_a_row,
        threshold,
        differing,
    }) = promotable(&store, binding, false).await
    else {
        panic!("a mismatch did not stop promotion");
    };
    assert_eq!((in_a_row, threshold), (4, 5));
    assert_eq!(differing[0].name, "customer");
    assert!(promote(&store, binding).await.is_err());

    // The fifth in a row makes it promotable, and a person's promotion is
    // recorded with its evidence.
    let fifth = keep(&store, compared(fence, binding, ShadowOutcome::Matched))
        .await
        .unwrap();
    assert!(matches!(
        promotable(&store, binding, false).await,
        Promotable::Yes { in_a_row: 5, threshold: 5, through_seq } if through_seq == fifth
    ));
    let promoted = promote(&store, binding).await.unwrap();
    assert_eq!(
        (promoted.matches, promoted.threshold, promoted.through_seq),
        (5, 5, fifth)
    );
    assert_eq!(promoted.promoted_by, principal(9));
    // Once only.
    assert!(matches!(
        promotable(&store, binding, false).await,
        Promotable::No(NotPromotable::Promoted(_))
    ));
    assert!(promote(&store, binding).await.is_err());

    // A binding whose steps changed has another digest, and has earned
    // nothing.
    assert_eq!(
        promotable(&store, Digest::of(b"the binding, changed"), false).await,
        Promotable::No(NotPromotable::NothingCompared)
    );

    // The report: by operation and digest, every count, the last mismatch's
    // differing outputs by name, and the times.
    let report = store
        .read(|tx| tx.shadow().report(&workspace(), &target(), None))
        .await
        .unwrap();
    assert_eq!(report.len(), 1);
    let standing = &report[0];
    assert_eq!(standing.binding, binding);
    assert_eq!(standing.mode.as_str(), "app_json_spare");
    assert_eq!(
        (
            standing.matches,
            standing.mismatches,
            standing.not_compared,
            standing.in_a_row
        ),
        (9, 1, 1, 5)
    );
    let last = standing.last_mismatch.as_ref().unwrap();
    assert_eq!(last.run, fence.run);
    assert_eq!(
        last.differing,
        vec![DifferingOutput {
            name: "customer".to_owned(),
            how: Difference::Differs
        }]
    );
    assert_eq!(standing.first_at, Timestamp::from_unix_millis(START));
    assert!(standing.promoted.is_some());
}

#[tokio::test]
async fn a_comparison_is_counted_only_beside_a_live_read_under_way() {
    let folder = Folder::new("shadow-counted");
    let store = on_disk(&folder).await;
    let binding = shadow_digest();

    // Of an operation that writes: the shadow of a write is not built, and
    // such a binding is never promotable through shadow.
    let fence = reading(&store, "a").await;
    let mut writing = compared(fence, binding, ShadowOutcome::Matched);
    writing.writes = true;
    let refused = keep(&store, writing).await.unwrap_err();
    assert_eq!(refused.code(), &codes::SHADOW_WRITE_NOT_BUILT);
    assert_eq!(
        promotable(&store, binding, true).await,
        Promotable::No(NotPromotable::WritesNotBuilt)
    );

    // Under a lease that is no longer the run's.
    store
        .write(Durability::Synced, move |tx| tx.ledger().new_epoch(fence.run))
        .await
        .unwrap();
    let lost = keep(&store, compared(fence, binding, ShadowOutcome::Matched))
        .await
        .unwrap_err();
    assert_eq!(lost.class(), FailureClass::PossibleCommit);

    // Beside a run that wrote, or one that has ended.
    let write_run = running(&store, 2, &["b"]).await;
    let refused = keep(&store, compared(write_run, binding, ShadowOutcome::Matched))
        .await
        .unwrap_err();
    assert_eq!(refused.code(), &codes::SHADOW_NOT_COUNTED);
    let ended = reading(&store, "c").await;
    store
        .write(Durability::Synced, move |tx| {
            tx.ledger().set_state(&ended, RunState::Completed, None)
        })
        .await
        .unwrap();
    let refused = keep(&store, compared(ended, binding, ShadowOutcome::Matched))
        .await
        .unwrap_err();
    assert_eq!(refused.code(), &codes::SHADOW_NOT_COUNTED);

    // The table refuses the same, and a change or removal of what was
    // counted, whoever sends the statement.
    let live = reading(&store, "d").await;
    keep(&store, compared(live, binding, ShadowOutcome::Matched))
        .await
        .unwrap();
    let raw = folder.raw();
    for statement in [
        "UPDATE shadow_comparisons SET outcome = 'match'",
        "DELETE FROM shadow_comparisons",
    ] {
        assert!(raw.execute(statement, []).is_err(), "{statement}");
    }
    let planted = raw.execute(
        "INSERT INTO shadow_comparisons
            (workspace_id, target, environment, operation, binding_digest, mode, position,
             against_digest, run_id, writes, outcome, at)
         SELECT workspace_id, target, environment, operation, binding_digest, mode, position,
                against_digest, ?1, 0, 'match', at FROM shadow_comparisons LIMIT 1",
        [ended.run.to_string()],
    );
    assert!(planted.is_err());

    // An offline test's own store counts nothing.
    let throwaway = Store::open(
        StoreConfig::throwaway()
            .with_clock(Arc::new(ManualClock::starting_at(
                Timestamp::from_unix_millis(START),
            )))
            .with_random(Counted::new()),
    )
    .await
    .unwrap();
    let vault = Arc::new(Vault::with_random(
        key_store(),
        Arc::new(throwaway.key_records()),
        Counted::new(),
    ));
    throwaway.attach(vault).await.unwrap();
    let offline = reading(&throwaway, "e").await;
    let refused = keep(&throwaway, compared(offline, binding, ShadowOutcome::Matched))
        .await
        .unwrap_err();
    assert_eq!(refused.code(), &codes::SHADOW_NOT_COUNTED);
}
