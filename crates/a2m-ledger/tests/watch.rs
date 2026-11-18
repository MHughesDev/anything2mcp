//! Watches: what the ledger keeps of a read repeated on a schedule a person
//! confirmed, and what it refuses (decision 0247). Each refusal is tried
//! through the calls and, where a statement can express it, through a
//! second connection to the database file.

#![allow(
    clippy::unwrap_used,
    reason = "this file holds only tests; clippy.toml allows unwrap in tests, which it tells by the #[test] attribute, and the helpers the tests share do not carry it"
)]

mod common;

use std::collections::BTreeSet;
use std::time::Duration;

use a2m_core::{
    CanonicalJson, Digest, EffectClass, Environment, Failure, FailureClass, FailureCode,
    OperationName, RecordKey, RunId, RunState, SurfaceName, Timestamp, Value, Values,
};
use a2m_ledger::{
    ChangeKind, CheckOutcome, Due, Durability, Missed, NewBlob, NewRun, NewWatch, Opened,
    RetentionClass, Store, ToolKind, WatchId, WatchKey, WatchReason, WatchRow, WatchShape,
    WatchState, codes,
};
use common::*;

fn operation() -> OperationName {
    OperationName::new("example_order_list").unwrap()
}

fn new_watch(effect: EffectClass) -> NewWatch {
    NewWatch {
        workspace: workspace(),
        target: target(),
        environment: Environment::new("test").unwrap(),
        operation: operation(),
        target_version: Digest::of(b"plan"),
        effect,
        input_hash: Digest::of(b"status=open"),
        input: None,
        ignore: BTreeSet::from(["seen_at".to_owned()]),
        surface: SurfaceName::new("orders").unwrap(),
        requests_per_check: 1,
        shape: WatchShape::List {
            list: "orders".to_owned(),
            key: vec!["id".to_owned()],
        },
        interval: Duration::from_secs(300),
        ends_at: Timestamp::from_unix_millis(START + 86_400_000),
        asked_by: principal(7),
        acts_for: principal(1),
        client_label: "example-agent".to_owned(),
        retention: RetentionClass::Long,
        condition: None,
    }
}

/// The published version the watches are asked for.
fn plan() -> Digest {
    Digest::of(b"plan")
}

fn the_read(caller: u8, input: &[u8]) -> NewRun {
    NewRun {
        tool_name: operation(),
        tool_kind: ToolKind::Operation,
        effect: EffectClass::Read,
        input_hash: Digest::of(input),
        ..new_run(EffectClass::Read, caller, "unused")
    }
}

async fn ask(store: &Store, new: NewWatch) -> Result<WatchRow, Failure> {
    store
        .write(Durability::Synced, move |tx| tx.watches().ask(&new))
        .await
}

async fn confirmed(store: &Store) -> WatchId {
    let watch = ask(store, new_watch(EffectClass::Read)).await.unwrap();
    store
        .write(Durability::Synced, move |tx| {
            tx.watches()
                .confirm(watch.id, principal(1), Duration::from_secs(7), &plan())
        })
        .await
        .unwrap();
    watch.id
}

/// Opens a read run and begins check `index` of the watch on it, in one
/// transaction, as the scheduler does.
async fn begin(store: &Store, watch: WatchId, index: u64, run: NewRun) -> Result<RunId, Failure> {
    store
        .write(Durability::Synced, move |tx| {
            let (id, opened) = match tx.ledger().open_run(run)? {
                Opened::New(fence) => (fence.run, true),
                Opened::Existing(id) => (id, false),
            };
            let due = Due {
                index,
                at: Timestamp::from_unix_millis(START),
                catch_up: false,
            };
            tx.watches().begin_check(watch, due, id, opened)?;
            Ok(id)
        })
        .await
}

/// Moves a run to `to`, with a failure when one is given, as the engine
/// does.
async fn finish(store: &Store, run: RunId, to: RunState, why: Option<Failure>) {
    store
        .write(Durability::Synced, move |tx| {
            let mut ledger = tx.ledger();
            let fence = ledger.new_epoch(run)?;
            ledger.set_state(&fence, RunState::Validating, None)?;
            if to != RunState::NeedsLogin {
                ledger.set_state(&fence, RunState::Running, None)?;
            }
            ledger.set_state(&fence, to, why.as_ref())
        })
        .await
        .unwrap();
}

fn order(id: &str, status: &str, seen_at: i64) -> Value {
    let values: Values = [
        ("id", Value::from(id)),
        ("status", Value::from(status)),
        ("seen_at", Value::from(seen_at)),
    ]
    .into_iter()
    .collect();
    Value::Record(values)
}

/// A read's result as the engine stores it: canonical JSON of its values,
/// a list of orders and a total beside it.
fn result_of(orders: Vec<Value>) -> Vec<u8> {
    let total = i64::try_from(orders.len()).unwrap();
    let values: Values = [
        ("orders", Value::List(orders)),
        ("total", Value::from(total)),
    ]
    .into_iter()
    .collect();
    CanonicalJson::encode(&values).unwrap().as_bytes().to_vec()
}

/// Completes a run with `result` stored as its result, as the engine does.
async fn complete_with(store: &Store, run: RunId, result: Option<Vec<u8>>) {
    let stored = match result {
        Some(plain) => Some(
            store
                .blobs()
                .put(NewBlob {
                    workspace: workspace(),
                    owner: a2m_vault::OwnerRef::service("engine").unwrap(),
                    kind: a2m_ledger::BlobKind::Result,
                    class: RetentionClass::Long,
                    plain,
                })
                .await
                .unwrap(),
        ),
        None => None,
    };
    store
        .write(Durability::Synced, move |tx| {
            let mut ledger = tx.ledger();
            let fence = ledger.new_epoch(run)?;
            ledger.set_state(&fence, RunState::Validating, None)?;
            ledger.set_state(&fence, RunState::Running, None)?;
            if let Some(stored) = stored {
                ledger.store_result(&fence, stored)?;
            }
            ledger.set_state(&fence, RunState::Completed, None)
        })
        .await
        .unwrap();
}

async fn running_check(store: &Store, watch: WatchId) -> u64 {
    store
        .read(move |tx| tx.watches().get(watch))
        .await
        .unwrap()
        .running_check
        .unwrap()
}

async fn end_with(
    store: &Store,
    watch: WatchId,
    seq: u64,
    result: Option<Vec<u8>>,
) -> Result<a2m_ledger::CheckEnded, Failure> {
    store
        .write(Durability::Synced, move |tx| {
            tx.watches().end_check(watch, seq, result.as_deref())
        })
        .await
}

/// One whole check: a run of the read that completes with `orders` as its
/// result, ended with that result's bytes.
async fn check(
    store: &Store,
    watch: WatchId,
    index: u64,
    orders: Vec<Value>,
) -> a2m_ledger::CheckEnded {
    let run = begin(store, watch, index, the_read(1, b"status=open"))
        .await
        .unwrap();
    let result = result_of(orders);
    complete_with(store, run, Some(result.clone())).await;
    let seq = running_check(store, watch).await;
    end_with(store, watch, seq, Some(result)).await.unwrap()
}

async fn changes(store: &Store, watch: WatchId) -> Vec<a2m_ledger::ChangeRow> {
    store
        .read(move |tx| tx.watches().changes(watch, 0, 1000))
        .await
        .unwrap()
}

fn refused<T: std::fmt::Debug>(result: Result<T, Failure>, code: &FailureCode) {
    let failure = result.unwrap_err();
    assert_eq!(failure.code(), code, "{failure:?}");
}

fn raw_refused(folder: &Folder, sql: &str) {
    let raw = folder.raw();
    let outcome = raw.execute_batch(sql);
    assert!(outcome.is_err(), "the store took: {sql}");
}

fn runs_in(folder: &Folder) -> i64 {
    folder
        .raw()
        .query_row("SELECT COUNT(*) FROM runs", [], |row| row.get(0))
        .unwrap()
}

#[tokio::test]
async fn a_watch_is_made_only_of_a_read() {
    let folder = Folder::new("watch-read");
    let store = on_disk(&folder).await;
    for effect in [EffectClass::Write, EffectClass::Irreversible] {
        refused(
            ask(&store, new_watch(effect)).await,
            &codes::WATCH_NOT_A_READ,
        );
    }
    let watch = ask(&store, new_watch(EffectClass::Read)).await.unwrap();
    assert_eq!(watch.state, WatchState::Asked);
    // Whoever sends the statement, the table holds reads only.
    raw_refused(
        &folder,
        &format!(
            "UPDATE watches SET effect = 'write' WHERE id = '{}'",
            watch.id
        ),
    );
}

#[tokio::test]
async fn a_watch_does_nothing_until_its_own_person_confirms_it() {
    let folder = Folder::new("watch-confirm");
    let store = on_disk(&folder).await;
    let watch = ask(&store, new_watch(EffectClass::Read)).await.unwrap();
    // No check before the confirmation, through the calls or the database.
    refused(
        begin(&store, watch.id, 0, the_read(1, b"status=open")).await,
        &codes::WATCH_NOT_CONFIRMED,
    );
    raw_refused(
        &folder,
        &format!(
            "UPDATE watches SET state = 'running', running_check = 0 WHERE id = '{}'",
            watch.id
        ),
    );
    // The agent that asked is not the person, and confirms nothing.
    let id = watch.id;
    refused(
        store
            .write(Durability::Synced, move |tx| {
                tx.watches()
                    .confirm(id, principal(7), Duration::ZERO, &plan())
            })
            .await,
        &codes::WATCH_NOT_YOURS,
    );
    raw_refused(
        &folder,
        &format!(
            "UPDATE watches SET confirmed_by = '{}', confirmed_at = 1, phase_ms = 0,
                    state = 'confirmed' WHERE id = '{id}'",
            principal(7)
        ),
    );
    let row = store
        .write(Durability::Synced, move |tx| {
            tx.watches()
                .confirm(id, principal(1), Duration::from_secs(3), &plan())
        })
        .await
        .unwrap();
    assert_eq!(row.state, WatchState::Confirmed);
    assert_eq!(row.phase, Some(Duration::from_secs(3)));
    let told = changes(&store, id).await;
    assert_eq!(told.len(), 1);
    assert_eq!(told[0].kind, ChangeKind::Confirmed);
    // Once only.
    raw_refused(
        &folder,
        &format!("UPDATE watches SET phase_ms = 5 WHERE id = '{id}'"),
    );
}

#[tokio::test]
async fn what_a_watch_reads_never_changes_and_no_check_runs_another_read() {
    let folder = Folder::new("watch-own");
    let store = on_disk(&folder).await;
    let watch = confirmed(&store).await;
    for column in [
        "input_hash = 'x'",
        "operation = 'example_order_delete'",
        "target_version_hash = 'x'",
        "interval_seconds = 1",
        "acts_for = 'p_00'",
        "ends_at = ends_at + 1",
    ] {
        raw_refused(
            &folder,
            &format!("UPDATE watches SET {column} WHERE id = '{watch}'"),
        );
    }
    // Another input, another caller, another operation, a write: refused,
    // and the transaction that opened the run is taken back with it.
    let before = runs_in(&folder);
    let other_operation = NewRun {
        tool_name: OperationName::new("example_order_get").unwrap(),
        ..the_read(1, b"status=open")
    };
    let a_write = NewRun {
        effect: EffectClass::Write,
        ..the_read(1, b"status=open")
    };
    for run in [
        the_read(1, b"status=closed"),
        the_read(2, b"status=open"),
        other_operation,
        a_write,
    ] {
        refused(
            begin(&store, watch, 0, run).await,
            &codes::WATCH_RUN_NOT_ITS_OWN,
        );
    }
    assert_eq!(runs_in(&folder), before);
    // Its own read is taken, once at a time.
    begin(&store, watch, 0, the_read(1, b"status=open"))
        .await
        .unwrap();
    refused(
        begin(&store, watch, 1, the_read(1, b"status=open")).await,
        &codes::WATCH_CHECK_RUNNING,
    );
    // A check row for a run that is not the watch's own is refused by the
    // database too.
    let stray: String = folder
        .raw()
        .query_row("SELECT run_id FROM watch_checks LIMIT 1", [], |row| {
            row.get(0)
        })
        .unwrap();
    raw_refused(
        &folder,
        &format!(
            "INSERT INTO watch_checks (watch_id, seq, due_index, due_at, catch_up, run_id,
                                       started_at, outcome)
             VALUES ('{watch}', 9, 9, 0, 0, '{stray}', 0, 'running')"
        ),
    );
}

#[tokio::test]
async fn checks_report_by_key_and_output_name_and_keep_no_value() {
    let folder = Folder::new("watch-compare");
    let store = on_disk(&folder).await;
    let watch = confirmed(&store).await;
    let first = check(
        &store,
        watch,
        0,
        vec![order("A-1", "open", 1), order("A-2", "open", 1)],
    )
    .await;
    // Completeness is not recorded, so no check is complete.
    assert_eq!(first.check.outcome, CheckOutcome::CutShort);
    assert_eq!(first.comparison, None);
    assert_eq!(first.check.records_seen, Some(2));
    // A record changed, one is new, one is not seen; `seen_at` moved too
    // and is ignored.
    let second = check(
        &store,
        watch,
        1,
        vec![
            order("A-1", "shipped-to-the-coast", 2),
            order("A-3", "open", 2),
        ],
    )
    .await;
    assert_eq!(second.state, WatchState::Confirmed);
    let counts = second.check.counts.unwrap();
    assert_eq!(
        (counts.added, counts.changed, counts.removed, counts.unseen),
        (1, 1, 0, 1)
    );
    let told = changes(&store, watch).await;
    let kinds: Vec<ChangeKind> = told.iter().map(|change| change.kind).collect();
    assert_eq!(
        kinds,
        vec![
            ChangeKind::Confirmed,
            ChangeKind::Added,
            ChangeKind::Changed,
            ChangeKind::Unseen
        ]
    );
    let changed = &told[2];
    assert_eq!(changed.outputs, vec!["status".to_owned()]);
    assert_eq!(changed.older_check, Some(0));
    let key = RecordKey::new(vec![Value::from("A-1")]).unwrap();
    let named = store
        .read(move |tx| tx.watches().key_of(&workspace(), &key))
        .await
        .unwrap();
    assert_eq!(changed.key, Some(named));
    // No value of what the read returned is in any watch table.
    let raw = folder.raw();
    for table in ["watches", "watch_checks", "watch_changes", "watch_baseline"] {
        let mut statement = raw.prepare(&format!("SELECT * FROM {table}")).unwrap();
        let columns = statement.column_count();
        let mut rows = statement.query([]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            for at in 0..columns {
                let text: Option<String> = row.get::<_, Option<String>>(at).unwrap_or(None);
                if let Some(text) = text {
                    for value in ["A-1", "A-3", "shipped-to-the-coast"] {
                        assert!(!text.contains(value), "{table} holds `{value}`");
                    }
                }
            }
        }
    }
    let baseline: i64 = raw
        .query_row("SELECT COUNT(*) FROM watch_baseline", [], |row| row.get(0))
        .unwrap();
    assert_eq!(baseline, 3, "a record not seen stays in the baseline");
}

#[tokio::test]
async fn a_completed_check_never_reports_a_removal_while_completeness_is_not_recorded() {
    let store = memory().await;
    let watch = confirmed(&store).await;
    let all = || vec![order("A-1", "open", 1), order("A-2", "open", 1)];
    check(&store, watch, 0, all()).await;
    for index in 1..=3 {
        let quiet = check(&store, watch, index, all()).await;
        assert!(quiet.comparison.unwrap().is_quiet());
    }
    assert_eq!(
        changes(&store, watch).await.len(),
        1,
        "only the confirmation"
    );
    // A completed run that returned one record fewer: the record is unseen,
    // never removed, and told once, however often the run completes so.
    for index in 4..=6 {
        let short = check(&store, watch, index, vec![order("A-1", "open", 1)]).await;
        assert_eq!(short.check.outcome, CheckOutcome::CutShort);
        assert_eq!(short.check.counts.unwrap().removed, 0);
    }
    let told = changes(&store, watch).await;
    let kinds: Vec<ChangeKind> = told.iter().map(|change| change.kind).collect();
    assert_eq!(kinds, vec![ChangeKind::Confirmed, ChangeKind::Unseen]);
    // An empty list removes nothing either.
    let empty = check(&store, watch, 7, Vec::new()).await;
    assert_eq!(empty.check.counts.unwrap().removed, 0);
    // When it comes back it is not new.
    let back = check(&store, watch, 8, all()).await;
    assert!(back.comparison.unwrap().is_quiet());
    let row = store.read(move |tx| tx.watches().get(watch)).await.unwrap();
    assert!(!row.baseline_complete);
}

#[tokio::test]
async fn a_check_whose_result_is_not_its_runs_own_is_refused() {
    let store = memory().await;
    let watch = confirmed(&store).await;
    let run = begin(&store, watch, 0, the_read(1, b"status=open"))
        .await
        .unwrap();
    let own = result_of(vec![order("A-1", "open", 1)]);
    complete_with(&store, run, Some(own.clone())).await;
    // Other bytes than the run's stored result: refused, and the check stays
    // under way.
    let other = result_of(vec![order("A-1", "open", 1), order("A-2", "open", 1)]);
    refused(
        end_with(&store, watch, 0, Some(other)).await,
        &codes::WATCH_RUN_NOT_ITS_OWN,
    );
    refused(
        end_with(&store, watch, 0, None).await,
        &FailureCode::SCHEMA_INVALID,
    );
    assert_eq!(running_check(&store, watch).await, 0);
    let ended = end_with(&store, watch, 0, Some(own)).await.unwrap();
    assert_eq!(ended.check.records_seen, Some(1));

    // A completed run that stored no result: nothing it is handed is its own.
    let run = begin(&store, watch, 1, the_read(1, b"status=open"))
        .await
        .unwrap();
    complete_with(&store, run, None).await;
    refused(
        end_with(&store, watch, 1, Some(result_of(Vec::new()))).await,
        &codes::WATCH_RUN_NOT_ITS_OWN,
    );

    // Its own result, not of the watch's shape: a gap, and the watch goes on.
    let other_watch = confirmed(&store).await;
    let run = begin(&store, other_watch, 0, the_read(1, b"status=open"))
        .await
        .unwrap();
    let not_a_list = CanonicalJson::encode(&Values::new().with("orders", "none"))
        .unwrap()
        .as_bytes()
        .to_vec();
    complete_with(&store, run, Some(not_a_list.clone())).await;
    let faulted = end_with(&store, other_watch, 0, Some(not_a_list))
        .await
        .unwrap();
    assert_eq!(faulted.check.outcome, CheckOutcome::Faulted);
    assert_eq!(
        faulted.check.why.map(|(_, code)| code),
        Some(codes::WATCH_RESULT_NOT_ITS_SHAPE)
    );
    assert_eq!(faulted.state, WatchState::Confirmed);
}

#[tokio::test]
async fn equal_values_of_two_records_have_different_digests() {
    let store = memory().await;
    let watch = confirmed(&store).await;
    check(
        &store,
        watch,
        0,
        vec![order("A-1", "open", 1), order("A-2", "open", 1)],
    )
    .await;
    let row = store.read(move |tx| tx.watches().get(watch)).await.unwrap();
    let baseline = store
        .read(move |tx| tx.watches().baseline(&row))
        .await
        .unwrap()
        .unwrap();
    let statuses: BTreeSet<Digest> = baseline
        .records
        .values()
        .map(|held| held.digests.outputs["status"])
        .collect();
    assert_eq!(baseline.records.len(), 2);
    assert_eq!(statuses.len(), 2, "one status, two records, two digests");
    // And the same values in another watch have other digests again.
    let second = confirmed(&store).await;
    check(
        &store,
        second,
        0,
        vec![order("A-1", "open", 1), order("A-2", "open", 1)],
    )
    .await;
    let row = store
        .read(move |tx| tx.watches().get(second))
        .await
        .unwrap();
    let other = store
        .read(move |tx| tx.watches().baseline(&row))
        .await
        .unwrap()
        .unwrap();
    let key = RecordKey::new(vec![Value::from("A-1")]).unwrap();
    let named: WatchKey = store
        .read(move |tx| tx.watches().key_of(&workspace(), &key))
        .await
        .unwrap();
    assert_ne!(
        baseline.records[&named].digests.whole,
        other.records[&named].digests.whole
    );
}

#[tokio::test]
async fn a_key_twice_in_one_check_is_a_gap_and_the_baseline_stays() {
    let store = memory().await;
    let watch = confirmed(&store).await;
    check(&store, watch, 0, vec![order("A-1", "open", 1)]).await;
    let twice = check(
        &store,
        watch,
        1,
        vec![order("A-1", "open", 1), order("A-1", "shipped", 1)],
    )
    .await;
    assert_eq!(twice.check.outcome, CheckOutcome::Faulted);
    assert_eq!(
        twice.check.why.map(|(_, code)| code),
        Some(codes::WATCH_KEY_REPEATED)
    );
    let row = store.read(move |tx| tx.watches().get(watch)).await.unwrap();
    assert_eq!(row.baseline_through, Some(0));
    let gap = changes(&store, watch).await.pop().unwrap();
    assert_eq!(gap.kind, ChangeKind::Gap);
    assert!(
        check(&store, watch, 2, vec![order("A-1", "open", 1)])
            .await
            .comparison
            .unwrap()
            .is_quiet()
    );
}

#[tokio::test]
async fn a_failed_check_is_a_gap_and_a_sign_in_pauses_the_watch() {
    let store = memory().await;
    let watch = confirmed(&store).await;
    let run = begin(&store, watch, 0, the_read(1, b"status=open"))
        .await
        .unwrap();
    // Until its run ends, the check cannot end.
    refused(
        store
            .write(Durability::Synced, move |tx| {
                tx.watches().end_check(watch, 0, None)
            })
            .await,
        &codes::WATCH_CHECK_UNFINISHED,
    );
    finish(
        &store,
        run,
        RunState::Failed,
        Some(stopped_by(FailureClass::Transient, "connection_lost")),
    )
    .await;
    let ended = store
        .write(Durability::Synced, move |tx| {
            tx.watches().end_check(watch, 0, None)
        })
        .await
        .unwrap();
    assert_eq!(ended.check.outcome, CheckOutcome::Failed);
    assert_eq!(ended.state, WatchState::Confirmed);
    let gap = changes(&store, watch).await.pop().unwrap();
    assert_eq!(gap.kind, ChangeKind::Gap);
    assert_eq!(
        gap.why,
        Some((
            FailureClass::Transient,
            FailureCode::fixed("connection_lost")
        ))
    );

    let run = begin(&store, watch, 1, the_read(1, b"status=open"))
        .await
        .unwrap();
    finish(&store, run, RunState::NeedsLogin, None).await;
    let ended = store
        .write(Durability::Synced, move |tx| {
            tx.watches().end_check(watch, 1, None)
        })
        .await
        .unwrap();
    assert_eq!(ended.state, WatchState::Paused);
    refused(
        begin(&store, watch, 2, the_read(1, b"status=open")).await,
        &codes::WATCH_PAUSED,
    );
    let row = store
        .write(Durability::Synced, move |tx| tx.watches().resume(watch))
        .await
        .unwrap();
    assert_eq!(row.state, WatchState::Confirmed);

    // A new published version needs the person again: no resume.
    let running = store
        .write(Durability::Synced, move |tx| {
            tx.watches()
                .version_changed(&workspace(), &target(), &Digest::of(b"plan 2"))
        })
        .await
        .unwrap();
    assert!(running.is_empty());
    let row = store.read(move |tx| tx.watches().get(watch)).await.unwrap();
    assert_eq!(row.state, WatchState::Paused);
    assert_eq!(row.reason, Some(WatchReason::VersionChanged));
    refused(
        store
            .write(Durability::Synced, move |tx| tx.watches().resume(watch))
            .await,
        &codes::WATCH_NEEDS_CONFIRMATION,
    );
}

#[tokio::test]
async fn missed_due_times_are_one_gap_and_never_go_back() {
    let folder = Folder::new("watch-missed");
    let store = on_disk(&folder).await;
    let watch = confirmed(&store).await;
    let missed = Missed {
        first: Timestamp::from_unix_millis(START),
        last: Timestamp::from_unix_millis(START + 600_000),
        count: 3,
    };
    store
        .write(Durability::Synced, move |tx| {
            tx.watches().missed(watch, 0, 3, missed)
        })
        .await
        .unwrap();
    let gap = changes(&store, watch).await.pop().unwrap();
    assert_eq!(gap.kind, ChangeKind::Gap);
    assert_eq!(gap.missed, Some(missed));
    assert_eq!(gap.why.unwrap().1, codes::WATCH_DUE_MISSED);
    // A due time handled is never checked.
    assert!(
        begin(&store, watch, 2, the_read(1, b"status=open"))
            .await
            .is_err()
    );
    raw_refused(
        &folder,
        &format!("UPDATE watches SET next_due_index = 0 WHERE id = '{watch}'"),
    );
}

#[tokio::test]
async fn ending_is_always_possible_and_leaves_the_history() {
    let folder = Folder::new("watch-end");
    let store = on_disk(&folder).await;
    // From `asked`.
    let asked = ask(&store, new_watch(EffectClass::Read)).await.unwrap().id;
    // From a check under way.
    let running = confirmed(&store).await;
    check(&store, running, 0, vec![order("A-1", "open", 1)]).await;
    begin(&store, running, 1, the_read(1, b"status=open"))
        .await
        .unwrap();
    // From `paused`.
    let paused = confirmed(&store).await;
    store
        .write(Durability::Synced, move |tx| {
            tx.watches().pause(paused, WatchReason::PausedByPerson)
        })
        .await
        .unwrap();
    for watch in [asked, running, paused] {
        let row = store
            .write(Durability::Synced, move |tx| {
                tx.watches().end(watch, WatchReason::EndedByPerson)
            })
            .await
            .unwrap();
        assert_eq!(row.state, WatchState::Ended);
        refused(
            store
                .write(Durability::Synced, move |tx| {
                    tx.watches().end(watch, WatchReason::EndedByPerson)
                })
                .await,
            &codes::WATCH_ENDED,
        );
    }
    let checks = store
        .read(move |tx| tx.watches().checks(running, None, 10))
        .await
        .unwrap();
    assert_eq!(checks.len(), 2);
    assert_eq!(checks[1].outcome, CheckOutcome::Failed);
    // The check that ending closed is a gap, then the end.
    let told: Vec<ChangeKind> = changes(&store, running)
        .await
        .iter()
        .map(|change| change.kind)
        .collect();
    assert_eq!(
        &told[told.len() - 2..],
        [ChangeKind::Gap, ChangeKind::Ended]
    );
    let raw = folder.raw();
    let digests: i64 = raw
        .query_row("SELECT COUNT(*) FROM watch_baseline", [], |row| row.get(0))
        .unwrap();
    assert_eq!(digests, 0, "an ended watch keeps no digests");
    for sql in [
        format!("DELETE FROM watches WHERE id = '{running}'"),
        format!("DELETE FROM watch_checks WHERE watch_id = '{running}'"),
        format!("DELETE FROM watch_changes WHERE watch_id = '{running}'"),
        format!("UPDATE watch_changes SET kind = 'added' WHERE watch_id = '{running}'"),
        format!(
            "UPDATE watches SET state = 'confirmed', reason_code = NULL WHERE id = '{running}'"
        ),
        format!("UPDATE watch_checks SET outcome = 'complete' WHERE watch_id = '{running}'"),
    ] {
        raw_refused(&folder, &sql);
    }
    // Nothing the watch holds started a run of its own: one run per check.
    assert_eq!(runs_in(&folder), 2);
}

#[tokio::test]
async fn an_asked_watch_confirmed_after_a_publish_is_refused_for_its_old_version() {
    let store = memory().await;
    let watch = ask(&store, new_watch(EffectClass::Read)).await.unwrap().id;
    // The target was published again before its person confirmed.
    let published = Digest::of(b"plan 2");
    refused(
        store
            .write(Durability::Synced, move |tx| {
                tx.watches()
                    .confirm(watch, principal(1), Duration::from_secs(3), &published)
            })
            .await,
        &codes::WATCH_NEEDS_CONFIRMATION,
    );
    let row = store.read(move |tx| tx.watches().get(watch)).await.unwrap();
    assert_eq!(row.state, WatchState::Asked);
    assert_eq!(row.confirmed, None);
}

#[tokio::test]
async fn a_version_change_ends_an_asked_watch_of_another_version() {
    let store = memory().await;
    let asked = ask(&store, new_watch(EffectClass::Read)).await.unwrap().id;
    let confirmed = confirmed(&store).await;
    let current = ask(
        &store,
        NewWatch {
            target_version: Digest::of(b"plan 2"),
            ..new_watch(EffectClass::Read)
        },
    )
    .await
    .unwrap()
    .id;
    store
        .write(Durability::Synced, move |tx| {
            tx.watches()
                .version_changed(&workspace(), &target(), &Digest::of(b"plan 2"))
        })
        .await
        .unwrap();
    let state = |id: WatchId| {
        let store = &store;
        async move { store.read(move |tx| tx.watches().get(id)).await.unwrap() }
    };
    let row = state(asked).await;
    assert_eq!(
        (row.state, row.reason),
        (WatchState::Ended, Some(WatchReason::VersionChanged))
    );
    let row = state(confirmed).await;
    assert_eq!(
        (row.state, row.reason),
        (WatchState::Paused, Some(WatchReason::VersionChanged))
    );
    // A watch asked for the version now published stays asked.
    assert_eq!(state(current).await.state, WatchState::Asked);
}

#[tokio::test]
async fn a_check_is_not_begun_at_or_after_the_end() {
    let (store, clock) = memory_clocked().await;
    let watch = confirmed(&store).await;
    let ends_at = Timestamp::from_unix_millis(START + 86_400_000);
    let due_at = |at: Timestamp, index: u64| {
        let store = &store;
        async move {
            store
                .write(Durability::Synced, move |tx| {
                    let (id, opened) = match tx.ledger().open_run(the_read(1, b"status=open"))? {
                        Opened::New(fence) => (fence.run, true),
                        Opened::Existing(id) => (id, false),
                    };
                    tx.watches().begin_check(
                        watch,
                        Due {
                            index,
                            at,
                            catch_up: false,
                        },
                        id,
                        opened,
                    )
                })
                .await
        }
    };
    // A due time at the end, computed before it: refused.
    refused(due_at(ends_at, 5).await, &codes::WATCH_ENDED);
    // Once the clock reaches the end, any due time: refused.
    clock.advance(Duration::from_millis(86_400_000));
    refused(
        due_at(Timestamp::from_unix_millis(START), 0).await,
        &codes::WATCH_ENDED,
    );
    let past = store
        .read(move |tx| {
            tx.watches()
                .past_end(Timestamp::from_unix_millis(START + 86_400_000))
        })
        .await
        .unwrap();
    assert_eq!(past.iter().map(|row| row.id).collect::<Vec<_>>(), [watch]);
    assert!(
        store
            .read(move |tx| tx.watches().past_end(Timestamp::from_unix_millis(START)))
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn ask_refuses_an_interval_below_the_floor() {
    let folder = Folder::new("watch-floor");
    let store = on_disk(&folder).await;
    for new in [
        NewWatch {
            interval: Duration::from_secs(30),
            ..new_watch(EffectClass::Read)
        },
        NewWatch {
            ends_at: Timestamp::from_unix_millis(START + 367 * 86_400_000),
            ..new_watch(EffectClass::Read)
        },
        NewWatch {
            requests_per_check: 0,
            ..new_watch(EffectClass::Read)
        },
        NewWatch {
            shape: WatchShape::List {
                list: "orders".to_owned(),
                key: Vec::new(),
            },
            ..new_watch(EffectClass::Read)
        },
    ] {
        let failure = ask(&store, new).await.unwrap_err();
        assert_eq!(
            (failure.class(), failure.code()),
            (FailureClass::Input, &FailureCode::SCHEMA_INVALID)
        );
    }
    // At the floor and the ceiling: taken.
    ask(
        &store,
        NewWatch {
            interval: Duration::from_secs(60),
            ends_at: Timestamp::from_unix_millis(START + 366 * 86_400_000),
            ..new_watch(EffectClass::Read)
        },
    )
    .await
    .unwrap();
    // Whoever sends the statement: a watch below the floor, past the
    // ceiling, with no surface or no cost is refused, and a watch's surface,
    // cost, and shape never change.
    let insert = |id: u8, interval: i64, ends_at: i64, surface: &str, requests: i64| {
        format!(
            "INSERT INTO watches
                (id, workspace_id, target, environment, operation, target_version_hash, effect,
                 input_hash, ignore_outputs, interval_seconds, ends_at, asked_by, acts_for,
                 client_label, asked_at, retention_class, state, state_since, surface,
                 requests_per_check, record_shape)
             VALUES ('watch_{id:032x}', 'w', 'example', 'test', 'example_order_list', 'h', 'read',
                     'h', '[]', {interval}, {ends_at}, 'p', 'p', 'l', 1000, 'long', 'asked', 1000,
                     '{surface}', {requests}, '{{\"whole\":true}}')"
        )
    };
    let day = 86_400_000_i64;
    folder
        .raw()
        .execute_batch(&insert(1, 60, 1000 + 366 * day, "orders", 1))
        .unwrap();
    for change in [
        "surface = 'other'",
        "requests_per_check = 9",
        "record_shape = '{\"key\":[\"id\"],\"list\":\"orders\"}'",
    ] {
        raw_refused(
            &folder,
            &format!("UPDATE watches SET {change} WHERE id = 'watch_{:032x}'", 1),
        );
    }
    for planted in [
        insert(2, 30, 1000 + day, "orders", 1),
        insert(3, 60, 1000 + 367 * day, "orders", 1),
        insert(4, 60, 1000 + day, "", 1),
        insert(5, 60, 1000 + day, "orders", 0),
    ] {
        raw_refused(&folder, &planted);
    }
}

#[tokio::test]
async fn two_watches_of_one_read_share_a_run_and_only_the_opener_is_marked() {
    let folder = Folder::new("watch-shared");
    let store = on_disk(&folder).await;
    let first = confirmed(&store).await;
    let second = confirmed(&store).await;
    // The first check opens the run; the second, due while it is
    // unfinished, is handed the same run by rule 6 and opens none.
    let run = begin(&store, first, 0, the_read(1, b"status=open"))
        .await
        .unwrap();
    assert_eq!(
        begin(&store, second, 0, the_read(1, b"status=open"))
            .await
            .unwrap(),
        run
    );
    assert_eq!(runs_in(&folder), 1);
    let (under_way, opened) = store
        .read(move |tx| {
            Ok((
                tx.watches().checks_of_run(run)?,
                tx.watches().run_opened_by_a_check(run)?,
            ))
        })
        .await
        .unwrap();
    assert!(opened);
    let mut marks: Vec<(WatchId, bool)> = under_way
        .iter()
        .map(|check| (check.watch, check.opened_run))
        .collect();
    marks.sort();
    let mut wanted = vec![(first, true), (second, false)];
    wanted.sort();
    assert_eq!(marks, wanted);
    // Ending the first watch closes its check; the second's is still under
    // way on the run, and the opener's mark stays with the run.
    store
        .write(Durability::Synced, move |tx| {
            tx.watches().end(first, WatchReason::EndedByPerson)
        })
        .await
        .unwrap();
    let (under_way, opened) = store
        .read(move |tx| {
            Ok((
                tx.watches().checks_of_run(run)?,
                tx.watches().run_opened_by_a_check(run)?,
            ))
        })
        .await
        .unwrap();
    assert!(opened);
    assert_eq!(under_way.len(), 1);
    assert_eq!(under_way[0].watch, second);
    // A run no check opened, such as the person's own call, is no check's.
    let theirs = store
        .write(Durability::Synced, move |tx| {
            match tx.ledger().open_run(the_read(1, b"status=closed"))? {
                Opened::New(fence) => Ok(fence.run),
                Opened::Existing(id) => Ok(id),
            }
        })
        .await
        .unwrap();
    assert!(
        !store
            .read(move |tx| tx.watches().run_opened_by_a_check(theirs))
            .await
            .unwrap()
    );
    // A run a check names was opened already: no second check opens it,
    // through the calls or the database, and the mark never changes.
    let third = confirmed(&store).await;
    refused(
        store
            .write(Durability::Synced, move |tx| {
                let due = Due {
                    index: 0,
                    at: Timestamp::from_unix_millis(START),
                    catch_up: false,
                };
                tx.watches().begin_check(third, due, run, true)
            })
            .await,
        &FailureCode::SCHEMA_INVALID,
    );
    let raw = folder.raw();
    raw.execute_batch(&format!(
        "UPDATE watches SET state = 'running', running_check = 0 WHERE id = '{third}'"
    ))
    .unwrap();
    let planted = raw
        .execute_batch(&format!(
            "INSERT INTO watch_checks (watch_id, seq, due_index, due_at, catch_up, run_id,
                                       started_at, outcome, opened_run)
             VALUES ('{third}', 0, 0, {START}, 0, '{run}', {START}, 'running', 1)"
        ))
        .unwrap_err();
    assert!(
        planted
            .to_string()
            .contains("opens only a run no check names yet"),
        "{planted}"
    );
    let changed = raw
        .execute_batch(&format!(
            "UPDATE watch_checks SET opened_run = 1 WHERE watch_id = '{second}'"
        ))
        .unwrap_err();
    assert!(changed.to_string().contains("never changes"), "{changed}");
}

/// A condition over the read's `total`, in the plan's form, as the registry
/// compiles it against the read's outputs.
fn total_reaches(at_least: i64) -> a2m_core::PlanExpr {
    let mut env = a2m_expr::TypeEnv::new(a2m_expr::LibraryVersion::CURRENT);
    env.declare(
        a2m_expr::names::RESULT,
        a2m_core::ValueType::Record(vec![a2m_core::FieldOf::required(
            "total",
            a2m_core::ValueType::Int,
        )]),
    )
    .unwrap();
    a2m_expr::compile(
        &format!("result.total >= {at_least}"),
        &env,
        Some(&a2m_core::ValueType::Bool),
    )
    .unwrap()
    .to_plan_expr()
    .unwrap()
}

async fn confirmed_until(store: &Store, condition: a2m_core::PlanExpr) -> WatchId {
    let watch = ask(
        store,
        NewWatch {
            condition: Some(condition),
            ..new_watch(EffectClass::Read)
        },
    )
    .await
    .unwrap();
    store
        .write(Durability::Synced, move |tx| {
            tx.watches()
                .confirm(watch.id, principal(1), Duration::from_secs(7), &plan())
        })
        .await
        .unwrap();
    watch.id
}

#[tokio::test]
async fn a_condition_that_holds_on_a_checks_own_result_ends_the_watch() {
    let folder = Folder::new("watch-condition");
    let store = on_disk(&folder).await;
    let watch = confirmed_until(&store, total_reaches(2)).await;
    let first = check(&store, watch, 0, vec![order("A-1", "open", 1)]).await;
    assert_eq!(
        first.state,
        WatchState::Confirmed,
        "one order: it does not hold"
    );
    // A second order: the check records what it found, then that the
    // condition held, and the watch ends, in the transaction that ended the
    // check.
    let second = check(
        &store,
        watch,
        1,
        vec![order("A-1", "open", 1), order("A-2", "open", 1)],
    )
    .await;
    assert_eq!(second.state, WatchState::Ended);
    assert_eq!(second.check.outcome, CheckOutcome::CutShort);
    let told = changes(&store, watch).await;
    let kinds: Vec<ChangeKind> = told.iter().map(|change| change.kind).collect();
    assert_eq!(
        kinds,
        [
            ChangeKind::Confirmed,
            ChangeKind::Added,
            ChangeKind::ConditionMet,
            ChangeKind::Ended
        ]
    );
    assert_eq!(told[2].check, Some(1));
    assert_eq!(told[3].reason, Some(WatchReason::ConditionMet));
    let row = store.read(move |tx| tx.watches().get(watch)).await.unwrap();
    assert_eq!(row.reason, Some(WatchReason::ConditionMet));
    assert_eq!(row.condition, Some(total_reaches(2)));
    // The store holds the end, and no digest of the ended watch.
    let raw = folder.raw();
    let stored: (String, String) = raw
        .query_row(
            "SELECT state, reason FROM watches WHERE id = ?1",
            [watch.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(stored, ("ended".to_owned(), "condition_met".to_owned()));
    let baseline: i64 = raw
        .query_row("SELECT COUNT(*) FROM watch_baseline", [], |row| row.get(0))
        .unwrap();
    assert_eq!(baseline, 0);
    // Whoever sends the statement: a watch's condition never changes, and
    // a condition is met only on a check of its watch.
    raw_refused(
        &folder,
        &format!("UPDATE watches SET condition = NULL WHERE id = '{watch}'"),
    );
    raw_refused(
        &folder,
        &format!(
            "INSERT INTO watch_changes (watch_id, kind, check_seq, at)
             VALUES ('{watch}', 'condition_met', 9, 1)"
        ),
    );
}

#[tokio::test]
async fn a_condition_that_cannot_be_answered_is_a_gap_and_the_baseline_stays() {
    let folder = Folder::new("watch-condition-unanswered");
    let store = on_disk(&folder).await;
    let watch = confirmed_until(&store, total_reaches(1)).await;
    let _ = check(&store, watch, 0, Vec::new()).await;
    // The next result holds its orders and no `total`: the condition reads
    // a value that is not there, and is never taken as false.
    let run = begin(&store, watch, 1, the_read(1, b"status=open"))
        .await
        .unwrap();
    let values: Values = [("orders", Value::List(vec![order("A-1", "open", 1)]))]
        .into_iter()
        .collect();
    let result = CanonicalJson::encode(&values).unwrap().as_bytes().to_vec();
    complete_with(&store, run, Some(result.clone())).await;
    let seq = running_check(&store, watch).await;
    let ended = end_with(&store, watch, seq, Some(result)).await.unwrap();
    assert_eq!(ended.state, WatchState::Confirmed);
    assert_eq!(ended.check.outcome, CheckOutcome::Faulted);
    assert_eq!(
        ended.check.why.as_ref().map(|(_, code)| code),
        Some(&codes::WATCH_CONDITION_UNANSWERED)
    );
    let told = changes(&store, watch).await;
    let kinds: Vec<ChangeKind> = told.iter().map(|change| change.kind).collect();
    assert_eq!(kinds, [ChangeKind::Confirmed, ChangeKind::Gap]);
    // The baseline is the first check's still: the one order is not taken
    // in from a check that was not compared.
    let row = store.read(move |tx| tx.watches().get(watch)).await.unwrap();
    assert_eq!(row.baseline_through, Some(0));
}

#[tokio::test]
async fn ask_refuses_a_condition_that_does_not_load_or_give_true_or_false() {
    let store = memory().await;
    let mut not_a_test = total_reaches(2);
    not_a_test.result = a2m_core::ValueType::Int;
    let mut not_canonical = total_reaches(2);
    not_canonical.source = "result.total   >=2".to_owned();
    for condition in [not_a_test, not_canonical] {
        let failure = ask(
            &store,
            NewWatch {
                condition: Some(condition),
                ..new_watch(EffectClass::Read)
            },
        )
        .await
        .unwrap_err();
        assert_eq!(failure.code(), &FailureCode::SCHEMA_INVALID, "{failure:?}");
    }
}

/// Migration 0026 builds `watches` and `watch_changes` again. Run once
/// more on a store that holds a watch with checks, changes, and a baseline,
/// it copies every row across, keeps every reference of the other tables
/// whole, and gives back tables the store goes on using.
#[tokio::test]
async fn building_the_watch_tables_again_keeps_every_row_and_reference() {
    let folder = Folder::new("watch-rebuild");
    let store = on_disk(&folder).await;
    let watch = confirmed(&store).await;
    let _ = check(&store, watch, 0, vec![order("A-1", "open", 1)]).await;
    let _ = check(&store, watch, 1, vec![order("A-2", "open", 1)]).await;
    let counted = |raw: &rusqlite::Connection| -> Vec<i64> {
        ["watches", "watch_checks", "watch_changes", "watch_baseline"]
            .iter()
            .map(|table| {
                raw.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap()
            })
            .collect()
    };
    let raw = folder.raw();
    let before = counted(&raw);
    assert!(before.iter().all(|count| *count > 0), "{before:?}");
    raw.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    let text = include_str!("../migrations/0026-ledger-a-watch-ends-when-its-condition-holds.sql");
    raw.execute_batch(&format!("BEGIN IMMEDIATE;\n{text}\nCOMMIT;"))
        .unwrap();
    assert_eq!(counted(&raw), before);
    let broken: i64 = raw
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(broken, 0);
    drop(raw);
    // The rules hold on the tables built again, and the store goes on.
    raw_refused(
        &folder,
        &format!("DELETE FROM watch_changes WHERE watch_id = '{watch}'"),
    );
    let _ = check(&store, watch, 2, vec![order("A-2", "shipped", 1)]).await;
    let ended = store
        .write(Durability::Synced, move |tx| {
            tx.watches().end(watch, WatchReason::EndedByPerson)
        })
        .await
        .unwrap();
    assert_eq!(ended.state, WatchState::Ended);
    let kinds: Vec<ChangeKind> = changes(&store, watch)
        .await
        .iter()
        .map(|change| change.kind)
        .collect();
    assert_eq!(kinds.last(), Some(&ChangeKind::Ended), "{kinds:?}");
}
