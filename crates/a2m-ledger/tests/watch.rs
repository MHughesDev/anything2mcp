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
    Digest, EffectClass, Environment, Failure, FailureClass, FailureCode, OperationName, RecordKey,
    RunId, RunState, Timestamp, Value, Values,
};
use a2m_ledger::{
    ChangeKind, CheckOutcome, Due, Durability, Missed, NewRun, NewWatch, Observed, ObservedRecord,
    Opened, RetentionClass, Store, ToolKind, WatchId, WatchReason, WatchRow, WatchState, codes,
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
        interval: Duration::from_secs(300),
        ends_at: Timestamp::from_unix_millis(START + 86_400_000),
        asked_by: principal(7),
        acts_for: principal(1),
        client_label: "example-agent".to_owned(),
        retention: RetentionClass::Long,
    }
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
                .confirm(watch.id, principal(1), Duration::from_secs(7))
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
            let id = match tx.ledger().open_run(run)? {
                Opened::New(fence) => fence.run,
                Opened::Existing(id) => id,
            };
            let due = Due {
                index,
                at: Timestamp::from_unix_millis(START),
                catch_up: false,
            };
            tx.watches().begin_check(watch, due, id)?;
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

fn order(id: &str, status: &str, seen_at: i64) -> ObservedRecord {
    let values: Values = [
        ("id", Value::from(id)),
        ("status", Value::from(status)),
        ("seen_at", Value::from(seen_at)),
    ]
    .into_iter()
    .collect();
    ObservedRecord {
        key: Some(RecordKey::new(vec![Value::from(id)]).unwrap()),
        values,
    }
}

/// One whole check: a run of the read that completes with `records`.
async fn check(
    store: &Store,
    watch: WatchId,
    index: u64,
    records: Vec<ObservedRecord>,
    complete: bool,
) -> a2m_ledger::CheckEnded {
    let run = begin(store, watch, index, the_read(1, b"status=open"))
        .await
        .unwrap();
    finish(store, run, RunState::Completed, None).await;
    let seq = store
        .read(move |tx| tx.watches().get(watch))
        .await
        .unwrap()
        .running_check
        .unwrap();
    let observed = Observed { records, complete };
    store
        .write(Durability::Synced, move |tx| {
            tx.watches().end_check(watch, seq, Some(&observed))
        })
        .await
        .unwrap()
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
                tx.watches().confirm(id, principal(7), Duration::ZERO)
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
                .confirm(id, principal(1), Duration::from_secs(3))
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
        true,
    )
    .await;
    assert_eq!(first.check.outcome, CheckOutcome::Complete);
    assert_eq!(first.comparison, None);
    assert_eq!(first.check.records_seen, Some(2));
    // A record changed, one is new, one is gone; `seen_at` moved too and is
    // ignored.
    let second = check(
        &store,
        watch,
        1,
        vec![
            order("A-1", "shipped-to-the-coast", 2),
            order("A-3", "open", 2),
        ],
        true,
    )
    .await;
    assert_eq!(second.state, WatchState::Confirmed);
    let counts = second.check.counts.unwrap();
    assert_eq!(
        (counts.added, counts.changed, counts.removed, counts.unseen),
        (1, 1, 1, 0)
    );
    let told = changes(&store, watch).await;
    let kinds: Vec<ChangeKind> = told.iter().map(|change| change.kind).collect();
    assert_eq!(
        kinds,
        vec![
            ChangeKind::Confirmed,
            ChangeKind::Added,
            ChangeKind::Changed,
            ChangeKind::Removed
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
    assert_eq!(baseline, 2, "the record proven gone left the baseline");
}

#[tokio::test]
async fn quiet_checks_record_no_change_and_a_list_cut_short_never_reports_a_removal() {
    let store = memory().await;
    let watch = confirmed(&store).await;
    let all = || vec![order("A-1", "open", 1), order("A-2", "open", 1)];
    check(&store, watch, 0, all(), true).await;
    for index in 1..=10 {
        let quiet = check(&store, watch, index, all(), true).await;
        assert!(quiet.comparison.unwrap().is_quiet());
    }
    assert_eq!(
        changes(&store, watch).await.len(),
        1,
        "only the confirmation"
    );
    // Cut short: the record not returned is unseen, never removed, and is
    // told once.
    let short = check(&store, watch, 11, vec![order("A-1", "open", 1)], false).await;
    assert_eq!(short.check.outcome, CheckOutcome::CutShort);
    check(&store, watch, 12, vec![order("A-1", "open", 1)], false).await;
    let told = changes(&store, watch).await;
    assert_eq!(told.len(), 2);
    assert_eq!(told[1].kind, ChangeKind::Unseen);
    // With the full list, it is removed.
    check(&store, watch, 13, vec![order("A-1", "open", 1)], true).await;
    let told = changes(&store, watch).await;
    assert_eq!(told.last().unwrap().kind, ChangeKind::Removed);
}

#[tokio::test]
async fn a_key_twice_in_one_check_is_a_gap_and_the_baseline_stays() {
    let store = memory().await;
    let watch = confirmed(&store).await;
    check(&store, watch, 0, vec![order("A-1", "open", 1)], true).await;
    let twice = check(
        &store,
        watch,
        1,
        vec![order("A-1", "open", 1), order("A-1", "shipped", 1)],
        true,
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
        check(&store, watch, 2, vec![order("A-1", "open", 1)], true)
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
    check(&store, running, 0, vec![order("A-1", "open", 1)], true).await;
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
    assert_eq!(
        changes(&store, running).await.last().unwrap().kind,
        ChangeKind::Ended
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
