//! A caller's own runs, listed, and a caller's idempotency keys (decision
//! 0160).

#![allow(
    clippy::unwrap_used,
    reason = "this file holds only tests; clippy.toml allows unwrap in tests, which it tells by the #[test] attribute, and the helpers the tests share do not carry it"
)]

mod common;

use a2m_core::{Digest, EffectClass, RunId, RunState, Timestamp};
use a2m_ledger::{CallerKey, Durability, Opened, RunQuery, Store};
use common::*;

fn query(caller: u8) -> RunQuery {
    RunQuery {
        workspace: workspace(),
        principal: principal(caller),
        states: Vec::new(),
        tool: None,
        target: None,
        since: None,
        caller_key: None,
        after: None,
        limit: 50,
    }
}

async fn open(store: &Store, caller: u8, input: &str) -> RunId {
    let new = new_run(EffectClass::Read, caller, input);
    store
        .write(Durability::Synced, move |tx| tx.ledger().open_run(new))
        .await
        .map(|opened| opened.run())
        .unwrap()
}

async fn listed(store: &Store, query: RunQuery) -> Vec<RunId> {
    store
        .read(move |tx| tx.ledger().runs(&query))
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.id)
        .collect()
}

#[tokio::test]
async fn a_listing_holds_the_callers_own_runs_newest_first_filtered_and_paged() {
    let store = memory().await;
    let mut mine = Vec::new();
    for input in ["a", "b", "c", "d"] {
        mine.push(open(&store, 1, input).await);
    }
    let theirs = open(&store, 2, "a").await;
    // Newest first, and nobody else's.
    let all = listed(&store, query(1)).await;
    let newest_first: Vec<RunId> = mine.iter().rev().copied().collect();
    assert_eq!(all, newest_first);
    assert!(!all.contains(&theirs));

    // Paged: a page ends, and the next starts after its last run.
    let mut page = query(1);
    page.limit = 3;
    let first = listed(&store, page.clone()).await;
    assert_eq!(first, newest_first[..3]);
    page.after = first.last().copied();
    assert_eq!(listed(&store, page.clone()).await, newest_first[3..]);
    // Another caller's run is no place to start a page from.
    page.after = Some(theirs);
    let refused = store
        .read(move |tx| tx.ledger().runs(&page))
        .await
        .unwrap_err();
    assert_eq!(refused.code().as_str(), "not_found");

    // By state, and by time.
    let mut queued = query(1);
    queued.states = vec![RunState::Completed];
    assert!(listed(&store, queued.clone()).await.is_empty());
    queued.states = vec![RunState::Completed, RunState::Queued];
    assert_eq!(listed(&store, queued).await.len(), 4);
    let mut later = query(1);
    later.since = Some(Timestamp::from_unix_millis(i64::MAX / 2));
    assert!(listed(&store, later).await.is_empty());
}

#[tokio::test]
async fn a_key_is_held_bound_once_and_found_only_by_its_own_caller() {
    let folder = Folder::new("caller-keys");
    let store = on_disk(&folder).await;
    let run = open(&store, 1, "a").await;
    let key = CallerKey::new("example-key-1").unwrap();
    let request = Digest::of(b"the call");
    let (k, r) = (key.clone(), request);
    let held = store
        .write(Durability::Synced, move |tx| {
            let mut ledger = tx.ledger();
            ledger.hold_caller_key(&workspace(), &principal(1), &k, &r)?;
            let before = ledger.caller_key(&workspace(), &principal(1), &k)?;
            ledger.bind_caller_key(&workspace(), &principal(1), &k, run)?;
            // Binding again to the same run changes nothing.
            ledger.bind_caller_key(&workspace(), &principal(1), &k, run)?;
            let after = ledger.caller_key(&workspace(), &principal(1), &k)?;
            let other = ledger.caller_key(&workspace(), &principal(2), &k)?;
            Ok((before, after, other))
        })
        .await
        .unwrap();
    assert_eq!(held.0.as_ref().unwrap().run, None);
    assert_eq!(held.1.as_ref().unwrap().run, Some(run));
    assert_eq!(held.1.unwrap().request, request);
    // Another person's key of the same text is another key.
    assert_eq!(held.2, None);

    // A bound key names no other run, is not released, and lists its run.
    let second = open(&store, 1, "b").await;
    let k = key.clone();
    let refused = store
        .write(Durability::Synced, move |tx| {
            tx.ledger()
                .bind_caller_key(&workspace(), &principal(1), &k, second)
        })
        .await
        .unwrap_err();
    assert_eq!(refused.code().as_str(), "ledger_state_unexpected");
    let k = key.clone();
    store
        .write(Durability::Synced, move |tx| {
            tx.ledger()
                .release_caller_key(&workspace(), &principal(1), &k)
        })
        .await
        .unwrap();
    let mut by_key = query(1);
    by_key.caller_key = Some(key.clone());
    assert_eq!(listed(&store, by_key).await, vec![run]);
    let mut by_key = query(2);
    by_key.caller_key = Some(key);
    assert!(listed(&store, by_key).await.is_empty());

    // The key is kept as a digest: its text is nowhere in the table.
    let kept: Vec<String> = {
        let raw = folder.raw();
        let mut statement = raw
            .prepare("SELECT key_digest || request_hash FROM caller_idempotency")
            .unwrap();
        statement
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    assert_eq!(kept.len(), 1);
    assert!(!kept[0].contains("example-key-1"));
}

#[test]
fn a_key_is_bounded_and_printable_and_its_debug_print_shows_nothing_of_it() {
    assert!(CallerKey::new("").is_err());
    assert!(CallerKey::new("has space").is_err());
    assert!(CallerKey::new(&"k".repeat(201)).is_err());
    assert!(CallerKey::new("caf\u{e9}").is_err());
    let key = CallerKey::new(&"k".repeat(200)).unwrap();
    assert!(!format!("{key:?}").contains('k'.to_string().repeat(2).as_str()));
    let refused = CallerKey::new("example token").unwrap_err();
    assert_eq!(refused.code().as_str(), "idempotency_key_invalid");
    assert!(!refused.message().contains("example token"));
}

#[tokio::test]
async fn a_held_key_finds_the_run_its_call_was_admitted_as() {
    let store = memory().await;
    let new = new_run(EffectClass::Read, 1, "a");
    let (tool, environment, input) = (
        new.tool_name.clone(),
        new.environment.clone(),
        new.input_hash,
    );
    let found = store
        .write(Durability::Synced, move |tx| {
            let since = tx.now();
            let Opened::New(fence) = tx.ledger().open_run(new)? else {
                unreachable!()
            };
            let found = tx.ledger().run_called_since(
                &workspace(),
                &principal(1),
                (&tool, &environment, &input),
                since,
            )?;
            let other = tx.ledger().run_called_since(
                &workspace(),
                &principal(2),
                (&tool, &environment, &input),
                since,
            )?;
            Ok((fence.run, found, other))
        })
        .await
        .unwrap();
    assert_eq!(Some(found.0), found.1);
    assert_eq!(found.2, None);
}
