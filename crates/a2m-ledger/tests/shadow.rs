//! A binding in shadow: its comparisons are counted by its digest; what makes
//! it promotable is the number of distinct inputs that matched the verified
//! binding that acts beside it, since the last mismatch, in each environment
//! its evidence must come from; and a person's promotion is recorded only on
//! that evidence (decisions 0207 and 0251).

#![allow(
    clippy::unwrap_used,
    reason = "this file holds only tests; clippy.toml allows unwrap in tests, which it tells by the #[test] attribute, and the helpers the tests share do not carry it"
)]

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;

use a2m_core::{
    Digest, EffectClass, Environment, FailureClass, FailureCode, Mode, OperationName, RunState,
    Timestamp, Values,
};
use a2m_ledger::{
    Difference, DifferingOutput, Durability, EnvironmentEvidence, Fence, NotPromotable, Opened,
    Promotable, ShadowComparison, ShadowOutcome, ShadowPromotion, ShadowSubject, ShadowThreshold,
    Store, StoreConfig, codes,
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

fn verified_digest() -> Digest {
    Digest::of(b"the verified binding")
}

fn env(name: &str) -> Environment {
    Environment::new(name).unwrap()
}

fn subject_in(binding: Digest, writes: bool, environments: &[&str]) -> ShadowSubject {
    ShadowSubject {
        workspace: workspace(),
        target: target(),
        operation: operation(),
        binding,
        writes,
        against: verified_digest(),
        environments: environments.iter().map(|name| env(name)).collect(),
    }
}

fn subject(binding: Digest, writes: bool) -> ShadowSubject {
    subject_in(binding, writes, &["test"])
}

/// A read run of its own in `environment`, `running`, as the engine moves
/// one.
async fn reading_in(store: &Store, input: &str, environment: &str) -> Fence {
    let input = input.to_owned();
    let environment = env(environment);
    store
        .write(Durability::Synced, move |tx| {
            let mut ledger = tx.ledger();
            let mut new = new_run(EffectClass::Read, 1, &input);
            new.environment = environment;
            let Opened::New(opened) = ledger.open_run(new)? else {
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

async fn reading(store: &Store, input: &str) -> Fence {
    reading_in(store, input, "test").await
}

fn order(id: &str) -> Values {
    Values::new().with("order_id", id)
}

/// A comparison beside the call of the order `id`.
fn on(fence: Fence, id: &str, outcome: ShadowOutcome) -> ShadowComparison {
    ShadowComparison {
        fence,
        operation: operation(),
        binding: shadow_digest(),
        mode: Mode::new("app_json_spare").unwrap(),
        position: 1,
        against: verified_digest(),
        writes: false,
        outcome,
        input: order(id),
        requests: 1,
    }
}

fn compared(fence: Fence, binding: Digest, outcome: ShadowOutcome) -> ShadowComparison {
    ShadowComparison {
        binding,
        ..on(fence, "A-100", outcome)
    }
}

async fn keep(store: &Store, comparison: ShadowComparison) -> Result<u64, a2m_core::Failure> {
    store
        .write(Durability::Synced, move |tx| {
            tx.shadow().record(&comparison)
        })
        .await
}

async fn answer(store: &Store, subject: ShadowSubject) -> Promotable {
    store
        .read(move |tx| {
            tx.shadow()
                .promotable(&subject, ShadowThreshold::default())
        })
        .await
        .unwrap()
}

async fn promotable(store: &Store, binding: Digest, writes: bool) -> Promotable {
    answer(store, subject(binding, writes)).await
}

async fn promote(
    store: &Store,
    binding: Digest,
) -> Result<a2m_ledger::PromotionRow, a2m_core::Failure> {
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

fn held(environment: &str, distinct_inputs: u32, matches: u32) -> EnvironmentEvidence {
    EnvironmentEvidence {
        environment: env(environment),
        distinct_inputs,
        matches,
    }
}

const ORDERS: [&str; 5] = ["A-100", "A-101", "A-102", "A-103", "A-104"];

#[tokio::test]
async fn five_matches_of_one_input_are_one_distinct_input_and_five_distinct_inputs_promote() {
    let store = memory().await;
    let fence = reading(&store, "a").await;
    let binding = shadow_digest();

    // Nothing compared: not promotable.
    assert_eq!(
        promotable(&store, binding, false).await,
        Promotable::No(NotPromotable::NothingCompared)
    );

    // Five matches on one input: one distinct input, not promotable.
    for _ in 0..5 {
        keep(&store, on(fence, "A-100", ShadowOutcome::Matched))
            .await
            .unwrap();
    }
    assert_eq!(
        promotable(&store, binding, false).await,
        Promotable::No(NotPromotable::TooFew {
            evidence: vec![held("test", 1, 5)],
            threshold: 5
        })
    );
    let refused = promote(&store, binding).await.unwrap_err();
    assert_eq!(refused.code(), &codes::SHADOW_NOT_PROMOTABLE);
    assert_eq!(refused.class(), FailureClass::Policy);
    assert!(refused.message().contains("1 distinct input(s) in `test`"));

    // A shadow not compared neither counts nor starts the count again.
    keep(
        &store,
        on(
            fence,
            "A-101",
            ShadowOutcome::NotCompared {
                class: FailureClass::Busy,
                code: FailureCode::fixed("shadow_time_exceeded"),
            },
        ),
    )
    .await
    .unwrap();

    // Four more inputs that match: five distinct, promotable.
    let mut last = 0;
    for id in &ORDERS[1..] {
        last = keep(&store, on(fence, id, ShadowOutcome::Matched))
            .await
            .unwrap();
    }
    assert_eq!(
        promotable(&store, binding, false).await,
        Promotable::Yes {
            evidence: vec![held("test", 5, 9)],
            threshold: 5,
            through_seq: last,
        }
    );
    let promoted = promote(&store, binding).await.unwrap();
    assert_eq!(
        (promoted.matches, promoted.threshold, promoted.through_seq),
        (5, 5, last)
    );
    assert_eq!(promoted.against, Some(verified_digest()));
    assert_eq!(promoted.evidence, vec![held("test", 5, 9)]);
    assert_eq!(promoted.promoted_by, principal(9));
    // Once only, and the row reads back as it was written.
    let Promotable::No(NotPromotable::Promoted(again)) = promotable(&store, binding, false).await
    else {
        panic!("a promoted binding was promotable again");
    };
    assert_eq!(again, promoted);
    assert!(promote(&store, binding).await.is_err());

    // A binding whose steps changed has another digest, and has earned
    // nothing.
    assert_eq!(
        promotable(&store, Digest::of(b"the binding, changed"), false).await,
        Promotable::No(NotPromotable::NothingCompared)
    );
}

#[tokio::test]
async fn a_mismatch_in_any_environment_starts_the_count_again() {
    let store = memory().await;
    let test = reading(&store, "a").await;
    let production = reading_in(&store, "b", "production").await;
    let binding = shadow_digest();
    for id in &ORDERS[..4] {
        keep(&store, on(test, id, ShadowOutcome::Matched))
            .await
            .unwrap();
    }
    // A mismatch in another environment, against another verified binding:
    // evidence against the binding is never narrowed.
    let mut elsewhere = on(production, "B-1", differs("customer"));
    elsewhere.against = Digest::of(b"an earlier verified binding");
    keep(&store, elsewhere).await.unwrap();
    keep(&store, on(test, "A-104", ShadowOutcome::Matched))
        .await
        .unwrap();
    let Promotable::No(NotPromotable::MismatchSince {
        evidence,
        threshold,
        differing,
    }) = promotable(&store, binding, false).await
    else {
        panic!("a mismatch did not stop promotion");
    };
    assert_eq!((evidence, threshold), (vec![held("test", 1, 1)], 5));
    assert_eq!(differing[0].name, "customer");
    for id in &ORDERS[..4] {
        keep(&store, on(test, id, ShadowOutcome::Matched))
            .await
            .unwrap();
    }
    assert!(matches!(
        promotable(&store, binding, false).await,
        Promotable::Yes { threshold: 5, .. }
    ));
}

#[tokio::test]
async fn matches_in_one_environment_do_not_promote_a_binding_for_another() {
    let store = memory().await;
    let test = reading(&store, "a").await;
    let binding = shadow_digest();
    for id in ORDERS {
        keep(&store, on(test, id, ShadowOutcome::Matched))
            .await
            .unwrap();
    }
    // Promotable on evidence from `test`, for `test`.
    assert!(matches!(
        answer(&store, subject_in(binding, false, &["test"])).await,
        Promotable::Yes { .. }
    ));
    // Not for `production`, which holds nothing; nor for both.
    assert_eq!(
        answer(&store, subject_in(binding, false, &["production"])).await,
        Promotable::No(NotPromotable::TooFew {
            evidence: vec![held("production", 0, 0)],
            threshold: 5
        })
    );
    let both = answer(&store, subject_in(binding, false, &["test", "production"])).await;
    assert_eq!(
        both,
        Promotable::No(NotPromotable::TooFew {
            evidence: vec![held("production", 0, 0), held("test", 5, 5)],
            threshold: 5
        })
    );
    // A question that names no environment is refused, not answered.
    let refused = store
        .read(move |tx| {
            tx.shadow()
                .promotable(&subject_in(binding, false, &[]), ShadowThreshold::default())
        })
        .await
        .unwrap_err();
    assert_eq!(refused.class(), FailureClass::Input);
    // Production's own five make it promotable there.
    let production = reading_in(&store, "b", "production").await;
    for id in ORDERS {
        keep(&store, on(production, id, ShadowOutcome::Matched))
            .await
            .unwrap();
    }
    assert!(matches!(
        answer(&store, subject_in(binding, false, &["test", "production"])).await,
        Promotable::Yes { .. }
    ));
}

#[tokio::test]
async fn matches_against_an_earlier_verified_binding_do_not_count() {
    let store = memory().await;
    let fence = reading(&store, "a").await;
    let binding = shadow_digest();
    let earlier = Digest::of(b"an earlier verified binding");
    for id in ORDERS {
        let mut comparison = on(fence, id, ShadowOutcome::Matched);
        comparison.against = earlier;
        keep(&store, comparison).await.unwrap();
    }
    assert_eq!(
        promotable(&store, binding, false).await,
        Promotable::No(NotPromotable::TooFew {
            evidence: vec![held("test", 0, 0)],
            threshold: 5
        })
    );
    // The report counts against the verified binding asked about, and, when
    // none is named, against the one the newest comparison was against.
    let asked = BTreeMap::from([(operation(), verified_digest())]);
    let report = store
        .read(move |tx| tx.shadow().report(&workspace(), &target(), None, &asked))
        .await
        .unwrap();
    let standing = &report[0];
    assert_eq!(standing.against, verified_digest());
    assert_eq!(
        (
            standing.environments[0].matches_since,
            standing.environments[0].against_counted,
            standing.environments[0].distinct_inputs
        ),
        (5, 0, 0)
    );
    let report = store
        .read(move |tx| {
            tx.shadow()
                .report(&workspace(), &target(), None, &BTreeMap::new())
        })
        .await
        .unwrap();
    assert_eq!(report[0].against, earlier);
    assert_eq!(report[0].environments[0].distinct_inputs, 5);
}

#[tokio::test]
async fn the_report_shows_counts_and_cost_and_never_an_input_or_its_digest() {
    let folder = Folder::new("shadow-report");
    let store = on_disk(&folder).await;
    let fence = reading(&store, "a").await;
    let binding = shadow_digest();
    for id in &ORDERS[..3] {
        keep(&store, on(fence, id, ShadowOutcome::Matched))
            .await
            .unwrap();
    }
    keep(&store, on(fence, "A-100", ShadowOutcome::Matched))
        .await
        .unwrap();
    let mismatch = keep(&store, on(fence, "A-103", differs("customer")))
        .await
        .unwrap();
    keep(&store, on(fence, "A-104", ShadowOutcome::Matched))
        .await
        .unwrap();
    let report = store
        .read(|tx| {
            tx.shadow()
                .report(&workspace(), &target(), None, &BTreeMap::new())
        })
        .await
        .unwrap();
    assert_eq!(report.len(), 1);
    let standing = &report[0];
    assert_eq!(standing.binding, binding);
    assert_eq!(
        (
            standing.matches,
            standing.mismatches,
            standing.not_compared,
            standing.in_a_row,
            standing.requests,
        ),
        (5, 1, 0, 1, 6)
    );
    let test = &standing.environments[0];
    assert_eq!(test.environment, env("test"));
    assert_eq!(
        (test.reads, test.requests, test.distinct_inputs),
        (6, 6, 1)
    );
    let last = standing.last_mismatch.as_ref().unwrap();
    assert_eq!((last.run, &last.environment), (fence.run, &env("test")));
    assert_eq!(standing.first_at, Timestamp::from_unix_millis(START));

    // Nothing the report or the run's events hold names an input or its
    // digest, keyed or plain.
    let shown = format!("{report:?}");
    let events = store
        .read(move |tx| tx.ledger().events(fence.run, 0, 100))
        .await
        .unwrap();
    let said: Vec<&str> = events
        .iter()
        .filter(|event| event.payload.contains("shadow_read"))
        .map(|event| event.payload.as_str())
        .collect();
    assert_eq!(said.len(), 6);
    assert!(said[0].contains("\"requests\":1"), "{}", said[0]);
    let kept: Vec<String> = folder
        .raw()
        .prepare("SELECT input_digest FROM shadow_comparisons ORDER BY seq")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(kept.len(), 6);
    // Equal inputs, equal digests; and the digest is keyed, not the plain
    // digest of the input.
    assert_eq!(kept[0], kept[3]);
    assert_ne!(kept[0], kept[1]);
    for id in ORDERS {
        assert_ne!(kept[0], order(id).digest().to_string());
        assert!(!shown.contains(id) && said.iter().all(|text| !text.contains(id)));
    }
    for digest in &kept {
        assert!(!shown.contains(digest.as_str()));
        assert!(said.iter().all(|text| !text.contains(digest.as_str())));
    }
    assert!(mismatch > 0);
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
        .write(Durability::Synced, move |tx| {
            tx.ledger().new_epoch(fence.run)
        })
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

    // The tables refuse the same, a change or removal of what was counted
    // or of a promotion, and a comparison that names no input, whoever
    // sends the statement.
    let live = reading(&store, "d").await;
    for id in ORDERS {
        keep(&store, on(live, id, ShadowOutcome::Matched))
            .await
            .unwrap();
    }
    promote(&store, binding).await.unwrap();
    let raw = folder.raw();
    for statement in [
        "UPDATE shadow_comparisons SET outcome = 'match'",
        "UPDATE shadow_comparisons SET input_digest = 'planted'",
        "UPDATE shadow_comparisons SET requests = 0",
        "DELETE FROM shadow_comparisons",
        "UPDATE shadow_promotions SET evidence = '[]'",
        "UPDATE shadow_promotions SET against_digest = NULL",
        "DELETE FROM shadow_promotions",
    ] {
        assert!(raw.execute(statement, []).is_err(), "{statement}");
    }
    for run in [ended.run, live.run] {
        let planted = raw.execute(
            "INSERT INTO shadow_comparisons
                (workspace_id, target, environment, operation, binding_digest, mode, position,
                 against_digest, run_id, writes, outcome, at)
             SELECT workspace_id, target, environment, operation, binding_digest, mode, position,
                    against_digest, ?1, 0, 'match', at FROM shadow_comparisons LIMIT 1",
            [run.to_string()],
        );
        assert!(planted.is_err(), "{run}");
    }

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
    let refused = keep(
        &throwaway,
        compared(offline, binding, ShadowOutcome::Matched),
    )
    .await
    .unwrap_err();
    assert_eq!(refused.code(), &codes::SHADOW_NOT_COUNTED);
}
