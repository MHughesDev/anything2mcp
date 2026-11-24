//! The store itself: opening and migrating, who owns which table, a restart,
//! events and the state rebuilt from them, blobs, findings and open
//! questions, published versions, the vault's keys, and the audit chain.

#![allow(
    clippy::unwrap_used,
    reason = "this file holds only tests; clippy.toml allows unwrap in tests, which it tells by the #[test] attribute, and the helpers the tests share do not carry it"
)]

mod common;

use std::sync::Arc;

use a2m_core::{
    Coverage, Digest, EffectClass, EvidenceLevel, FailureClass, Finding, FindingKind, ProducedBy,
    RecordState, RunState, Source, Subject, Timestamp, UnknownReason, Version,
};
use a2m_ledger::{
    Acted, AuditEntry, BlobKind, BlobRef, ChainCheck, Durability, NewBlob, NewUnknown, Owner,
    PublishedVersion, ReadBack, RetentionClass, Settlement, Store, StoreConfig, UnknownStatus,
    codes, migrations, replay, table_owner,
};
use a2m_vault::{Generation, KeyId, KeyRecord, KeyRecords, KeyRef, Layer, OwnerRef, Wrapped};
use common::*;

// ---- opening, migrations, owners ----

#[tokio::test]
async fn every_table_has_exactly_one_owner_and_the_migrations_create_what_they_declare() {
    let store = memory().await;
    // A table its crate builds again is declared by each migration that
    // creates it, always by the same owner.
    let declared: Vec<(String, Owner)> = migrations()
        .unwrap()
        .iter()
        .flat_map(|migration| {
            migration
                .tables
                .iter()
                .map(|table| (table.clone(), migration.owner))
        })
        .collect::<std::collections::BTreeMap<_, _>>()
        .into_iter()
        .collect();
    assert!(declared.len() >= 12);
    // The first line of each file names a crate; the file's name says whose it is.
    for migration in migrations().unwrap() {
        let short = migration.owner.crate_name().trim_start_matches("a2m-");
        assert!(
            migration.name.contains(short),
            "{} is not named for its owner",
            migration.name
        );
    }
    let held = store.tables().await.unwrap();
    for (table, owner) in &held {
        assert!(owner.is_some(), "table `{table}` has no owner");
    }
    for (table, owner) in &declared {
        assert_eq!(table_owner(table), Some(*owner));
        assert!(
            held.iter().any(|(name, _)| name == table),
            "`{table}` was not created"
        );
    }
    // Nothing is in the store that no migration declares, but the store's own list.
    assert_eq!(held.len(), declared.len() + 1);
    assert_eq!(table_owner("approvals"), Some(Owner::Policy));
    assert_eq!(table_owner("runs"), Some(Owner::Ledger));
    assert_eq!(table_owner("no_such_table"), None);
}

#[tokio::test]
async fn a_migration_applied_twice_is_a_no_op_and_a_store_from_the_future_is_refused() {
    let folder = Folder::new("migrate");
    let applied = |folder: &Folder| -> Vec<(i64, String)> {
        let raw = folder.raw();
        let mut statement = raw
            .prepare("SELECT number, checksum FROM schema_migrations ORDER BY number")
            .unwrap();
        let rows = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap();
        rows.map(Result::unwrap).collect()
    };
    let keys = key_store();
    drop(open_with(Some(folder.path()), Arc::clone(&keys)).await);
    let first = applied(&folder);
    assert_eq!(first.len(), migrations().unwrap().len());

    // Opening again applies nothing and changes nothing.
    drop(open_with(Some(folder.path()), keys).await);
    assert_eq!(applied(&folder), first);

    // An applied migration that is not the one this engine carries.
    let raw = folder.raw();
    raw.execute(
        "UPDATE schema_migrations SET checksum = 'changed' WHERE number = 1",
        [],
    )
    .unwrap();
    let changed = Store::open(StoreConfig::in_dir(folder.path()))
        .await
        .unwrap_err();
    assert_eq!(changed.code(), &codes::STORE_VERSION_UNSUPPORTED);
    raw.execute(
        "UPDATE schema_migrations SET checksum = ?1 WHERE number = 1",
        [&first[0].1],
    )
    .unwrap();

    // A migration this engine does not carry: a newer engine wrote the store.
    raw.execute(
        "INSERT INTO schema_migrations VALUES (9999, '9999-later.sql', 'a2m-ledger', 'x', 0)",
        [],
    )
    .unwrap();
    let newer = Store::open(StoreConfig::in_dir(folder.path()))
        .await
        .unwrap_err();
    assert_eq!(newer.class(), FailureClass::Busy);
    assert_eq!(newer.code(), &codes::STORE_VERSION_UNSUPPORTED);
    // It was left as it was.
    assert_eq!(applied(&folder).len(), first.len() + 1);
}

/// Every table, index, and trigger the database holds, by kind and name.
fn schema(raw: &rusqlite::Connection) -> Vec<(String, String)> {
    let mut statement = raw
        .prepare("SELECT type, name FROM sqlite_master ORDER BY type, name")
        .unwrap();
    let rows = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap();
    rows.map(Result::unwrap).collect()
}

fn recorded(raw: &rusqlite::Connection) -> i64 {
    raw.query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
        row.get(0)
    })
    .unwrap()
}

#[tokio::test]
async fn a_half_migrated_store_is_refused_and_left_as_it_was() {
    // Section 224: the engine refuses to start on a half-migrated store. A
    // migration runs in one transaction with the row that records it, so
    // an interruption leaves neither, and the next start applies it.
    let carried = migrations().unwrap();
    let middle = i64::from(carried[carried.len() / 2].number);

    // A store that holds what the later migrations made, without the rows
    // that record them: it was half migrated outside this engine's order,
    // or changed by another program.
    let folder = Folder::new("half");
    drop(open_with(Some(folder.path()), key_store()).await);
    let raw = folder.raw();
    raw.execute("DELETE FROM schema_migrations WHERE number >= ?1", [middle])
        .unwrap();
    let (before, rows) = (schema(&raw), recorded(&raw));
    let refused = Store::open(StoreConfig::in_dir(folder.path()))
        .await
        .unwrap_err();
    assert_eq!(refused.class(), FailureClass::Busy, "{refused:?}");
    assert_eq!(
        refused.code(),
        &codes::STORE_VERSION_UNSUPPORTED,
        "{refused:?}"
    );
    assert!(refused.message().contains("half migrated"), "{refused:?}");
    // Nothing was applied: the store is as it was.
    assert_eq!(schema(&raw), before);
    assert_eq!(recorded(&raw), rows);

    // A store that holds a later migration without an earlier one.
    let folder = Folder::new("gap");
    drop(open_with(Some(folder.path()), key_store()).await);
    let raw = folder.raw();
    raw.execute("DELETE FROM schema_migrations WHERE number = ?1", [middle])
        .unwrap();
    let (before, rows) = (schema(&raw), recorded(&raw));
    let refused = Store::open(StoreConfig::in_dir(folder.path()))
        .await
        .unwrap_err();
    assert_eq!(
        refused.code(),
        &codes::STORE_VERSION_UNSUPPORTED,
        "{refused:?}"
    );
    assert!(
        refused.message().contains("without an earlier"),
        "{refused:?}"
    );
    assert_eq!(schema(&raw), before);
    assert_eq!(recorded(&raw), rows);
}

#[tokio::test]
async fn a_damaged_database_file_fails_the_integrity_check_at_start() {
    let folder = Folder::new("damaged");
    drop(open_with(Some(folder.path()), key_store()).await);
    // Everything the store wrote is moved from its log into the file, so
    // that the file holds the whole database whenever the store's own
    // threads finish closing.
    let raw = folder.raw();
    raw.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
        .unwrap();
    drop(raw);
    let file = folder.path().join(a2m_ledger::DATABASE_FILE_NAME);
    let mut bytes = std::fs::read(&file).unwrap();
    // The first page, the file's header, is left whole; every byte of the
    // pages after it is overwritten, as a failing disk would leave them.
    let page = usize::from(u16::from_be_bytes([bytes[16], bytes[17]]));
    assert!(bytes.len() > 3 * page, "{} bytes", bytes.len());
    for byte in &mut bytes[page..] {
        *byte = 0x5a;
    }
    std::fs::write(&file, &bytes).unwrap();

    let refused = Store::open(StoreConfig::in_dir(folder.path()))
        .await
        .unwrap_err();
    assert_eq!(refused.class(), FailureClass::Busy, "{refused:?}");
    assert_eq!(refused.code(), &codes::STORE_CORRUPT, "{refused:?}");
    // The file is kept as it is, for its restore.
    assert_eq!(std::fs::read(&file).unwrap(), bytes);
}

#[tokio::test]
async fn a_crate_reaches_its_own_tables_and_no_others() {
    let store = memory().await;
    let fence = previewed(&store, 1, &["a"]).await;

    // The policy engine's stand-in writes and reads its own table.
    let approval = store
        .write(Durability::Synced, move |tx| {
            record_approval(tx, fence.run, &diff_hash())
        })
        .await
        .unwrap();
    let found = store
        .read(move |tx| {
            policy_tables(tx).query_first(
                "SELECT run_id, used_at FROM approvals WHERE id = ?1",
                &[approval.to_string().into()],
            )
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.text(0).unwrap(), fence.run.to_string());
    assert_eq!(found.opt_int(1).unwrap(), None);

    // It cannot read or change a table of the ledger, of another crate, or
    // the database's own; and it cannot end the transaction it was handed.
    for statement in [
        "SELECT state FROM runs",
        "UPDATE runs SET state = 'running'",
        "DELETE FROM run_events",
        "INSERT INTO audit SELECT * FROM audit",
        "SELECT name FROM sqlite_master",
        "COMMIT",
        "PRAGMA foreign_keys = OFF",
        "DROP TABLE approvals",
        "CREATE TABLE mine (id TEXT)",
    ] {
        let refused = store
            .write(Durability::Synced, move |tx| {
                policy_tables(tx).execute(statement, &[])
            })
            .await
            .unwrap_err();
        assert_eq!(refused.code(), &codes::TABLE_NOT_OWNED, "{statement}");
        assert_eq!(refused.class(), FailureClass::UnknownVariant);
    }
    // Another crate cannot reach the policy engine's table, not even by
    // saying it is the policy engine, and nobody reaches the ledger's tables
    // by saying they are the ledger.
    for owner in [
        Owner::Policy,
        Owner::Auth,
        Owner::Sched,
        Owner::Engine,
        Owner::Actions,
        Owner::Ledger,
    ] {
        let refused = store
            .read(move |tx| tx.tables(owner).query("SELECT id FROM approvals", &[]))
            .await
            .unwrap_err();
        assert_eq!(refused.code(), &codes::TABLE_NOT_OWNED, "{owner:?}");
    }

    // A read cannot write, and a failure or a panic inside a transaction
    // takes all of it back and leaves the store working.
    let read_only = store
        .read(move |tx| policy_tables(tx).execute("DELETE FROM approvals", &[]))
        .await
        .unwrap_err();
    assert_eq!(read_only.code(), &codes::LEDGER_STATE_UNEXPECTED);
    let panicked = store
        .write(
            Durability::Synced,
            move |tx| -> Result<(), a2m_core::Failure> {
                policy_tables(tx).execute("DELETE FROM approvals", &[])?;
                panic!("inside a transaction");
            },
        )
        .await
        .unwrap_err();
    assert_eq!(panicked.code(), &codes::STORE_UNAVAILABLE);
    let still = store
        .read(move |tx| policy_tables(tx).query("SELECT id FROM approvals", &[]))
        .await
        .unwrap();
    assert_eq!(still.len(), 1);
    let health = store.health().await;
    assert!(health.can_commit && health.vault_attached);
}

/// Decision 0145: the policy engine's tables are reached only with the key
/// the store hands out once, and a key of one store opens no other.
#[tokio::test]
async fn the_policy_engines_tables_open_only_with_the_key_the_store_hands_out_once() {
    // Stores whose key no stand-in took.
    let store = Store::open(StoreConfig::in_memory()).await.unwrap();
    let other = Store::open(StoreConfig::in_memory()).await.unwrap();
    let key = store.policy_key().unwrap();
    let again = store.policy_key().unwrap_err();
    assert_eq!(again.code(), &codes::TABLE_NOT_OWNED);
    let theirs = other.policy_key().unwrap();

    let counted = store
        .read(move |tx| {
            tx.policy_tables(&key)?
                .query_first("SELECT COUNT(*) FROM approvals", &[])
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(counted.int(0).unwrap(), 0);
    let refused = store
        .read(move |tx| {
            tx.policy_tables(&theirs)?
                .query("SELECT id FROM approvals", &[])
        })
        .await
        .unwrap_err();
    assert_eq!(refused.code(), &codes::TABLE_NOT_OWNED);
}

/// Decision 0145: no crate but the policy engine names the key to its
/// tables in its source. A crate's tests reach them through the `testing`
/// feature instead.
#[test]
fn no_crate_but_the_policy_engine_names_the_policy_key() {
    let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let mut named = Vec::new();
    let mut stack = vec![crates];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if path.is_dir() {
                // Only the sources of a crate: not its tests, and not the
                // two crates that make and take the key.
                let skipped = matches!(
                    name.as_str(),
                    "tests" | "target" | "a2m-ledger" | "a2m-policy"
                );
                if !skipped {
                    stack.push(path);
                }
            } else if name.ends_with(".rs") {
                let text = std::fs::read_to_string(&path).unwrap();
                if text.contains("PolicyKey") || text.contains("policy_key(") {
                    named.push(path.display().to_string());
                }
            }
        }
    }
    assert!(
        named.is_empty(),
        "these name the policy engine's key: {named:?}"
    );
}

// ---- a restart ----

/// A value sealed by one engine opens for the next engine over the same
/// store and the same root key, and for no engine once the root is gone
/// (spec 216; the key-store rows of 224). The sealed bytes are kept in a
/// file, as a caller keeps them, and the key records are in the store.
#[tokio::test]
async fn a_sealed_value_survives_a_restart_over_the_store_and_opens_nothing_without_the_root() {
    use a2m_vault::{OwnerRef, SealContext, Sealed, codes as vault_codes};

    const VALUE: &str = "example-sealed-value-4c1e9";
    let cx = SealContext::credential(
        workspace(),
        OwnerRef::person("ana").unwrap(),
        &target(),
        "token",
    )
    .unwrap();
    let folder = Folder::new("sealed-restart");
    let keys = key_store();
    let kept = folder.path().join("sealed.bin");

    let (store, vault) = open_with(Some(folder.path()), Arc::clone(&keys)).await;
    let sealed = vault.seal(&cx, VALUE.as_bytes()).await.unwrap();
    std::fs::write(&kept, sealed.to_bytes()).unwrap();
    // The envelope holds nothing readable.
    assert!(!String::from_utf8_lossy(&sealed.to_bytes()).contains(VALUE));
    drop(vault);
    drop(store);

    // The next engine reads the envelope back and opens it.
    let (store, vault) = open_with(Some(folder.path()), Arc::clone(&keys)).await;
    let sealed = Sealed::from_bytes(&std::fs::read(&kept).unwrap()).unwrap();
    let opened = vault.open(&cx, &sealed).await.unwrap();
    assert_eq!(opened.expose(), VALUE.as_bytes());
    drop(vault);
    drop(store);

    // The root is deleted from the key store: nothing opens, by name, and
    // the store's files do not hold the value either.
    keys.remove_root();
    let store = Store::open(StoreConfig::in_dir(folder.path()))
        .await
        .unwrap();
    let vault = Arc::new(a2m_vault::Vault::new(
        Arc::clone(&keys) as Arc<dyn a2m_vault::KeyStore>,
        Arc::new(store.key_records()),
    ));
    let refused = vault.open(&cx, &sealed).await.unwrap_err();
    assert_eq!(refused.code(), &vault_codes::KEY_STORE_ENTRY_MISSING);
    // Nor does the engine start over it: attaching asks for the digest key.
    let unattached = store.attach(Arc::clone(&vault)).await.unwrap_err();
    assert_eq!(unattached.code(), &vault_codes::KEY_STORE_ENTRY_MISSING);
    // The store is closed before its files are searched: on Windows its
    // folder lock is a lock on the file's bytes, which no other reader may
    // read while it is held. Every file is searched, the lock file among them.
    drop(vault);
    store.release_folder();
    drop(store);
    let mut files = Vec::new();
    let mut folders = vec![folder.path().to_path_buf()];
    while let Some(next) = folders.pop() {
        for entry in std::fs::read_dir(next).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                folders.push(path);
            } else {
                files.push(std::fs::read(&path).unwrap());
            }
        }
    }
    for bytes in files {
        assert!(
            !String::from_utf8_lossy(&bytes).contains(VALUE),
            "the value is in a file"
        );
    }
}

#[tokio::test]
async fn after_a_restart_everything_is_there_and_a_run_stopped_mid_write_is_found() {
    let folder = Folder::new("restart");
    let keys = key_store();
    let (store, vault) = open_with(Some(folder.path()), Arc::clone(&keys)).await;
    let fence = running(&store, 1, &["done", "mid", "todo"]).await;
    let proof = evidence(&store, "the values read back").await;
    let given = evidence(&store, "the outputs the read-back showed").await;

    // One record is written and verified; the second has its intent stored
    // when the process ends. The verified record's outputs are those its
    // read-back showed, in place of those stored with its `acted` mark.
    lock(&store, fence, 0).await.unwrap();
    let token = intent(&store, fence, 0, "done").await.unwrap();
    store
        .write(Durability::Synced, move |tx| {
            let acted = tx.ledger().mark_acted(
                token,
                Acted {
                    assigned_key: Some("SO-1001".to_owned()),
                    outputs: Some(proof),
                },
            )?;
            let read = ReadBack {
                digest: intended("done"),
                after: Some(proof),
                evidence: proof,
                outputs: Some(given),
            };
            tx.ledger().mark_verified(&acted, read)
        })
        .await
        .unwrap();
    lock(&store, fence, 1).await.unwrap();
    let token = intent(&store, fence, 1, "mid").await.unwrap();
    let stamp = token.idempotency_token().to_owned();
    let events_before = store
        .read(move |tx| tx.ledger().events(fence.run, 0, 1000))
        .await
        .unwrap();
    drop(token);
    drop(vault);
    drop(store);

    let (store, _vault) = open_with(Some(folder.path()), keys).await;
    let run = store
        .read(move |tx| tx.ledger().run(fence.run))
        .await
        .unwrap();
    assert_eq!(run.state, RunState::Running);
    assert_eq!(run.epoch, fence.epoch);
    assert_eq!(run.diff_hash, Some(diff_hash()));
    assert!(run.approval.is_some());
    let events = store
        .read(move |tx| tx.ledger().events(fence.run, 0, 1000))
        .await
        .unwrap();
    assert_eq!(events, events_before);
    let records = store
        .read(move |tx| tx.ledger().records(fence.run))
        .await
        .unwrap();
    assert_eq!(records[0].state, RecordState::Verified);
    assert_eq!(records[0].outputs, Some(given));

    let records = store
        .read(move |tx| tx.ledger().records(fence.run))
        .await
        .unwrap();
    let states: Vec<RecordState> = records.iter().map(|record| record.state).collect();
    assert_eq!(
        states,
        [
            RecordState::Verified,
            RecordState::Intent,
            RecordState::Planned
        ]
    );
    assert_eq!(records[0].assigned_key.as_deref(), Some("SO-1001"));
    assert_eq!(records[0].evidence, Some(proof));

    // The reconcile query finds the record that was mid-write, with the
    // token it would have been sent under.
    let doubtful = store
        .read(move |tx| tx.ledger().in_doubt(fence.run))
        .await
        .unwrap();
    assert_eq!(doubtful.len(), 1);
    assert_eq!(doubtful[0].key, key("mid"));
    assert_eq!(doubtful[0].idempotency_token, stamp);
    assert_eq!(doubtful[0].intent_epoch, Some(fence.epoch));

    // The lock on it is still there; the evidence still opens; and a run
    // that resumes cannot write the verified record again.
    let locks = store
        .read(move |tx| tx.ledger().locks(fence.run))
        .await
        .unwrap();
    assert_eq!(locks.len(), 1);
    assert_eq!(locks[0].key_hash, doubtful[0].key_hash);
    let opened = store.blobs().get(&proof).await.unwrap();
    assert_eq!(opened.expose(), b"the values read back");
    let again = intent(&store, fence, 0, "done").await.unwrap_err();
    assert_eq!(again.code(), &codes::LEDGER_STATE_UNEXPECTED);
    let planned_again = store
        .write(Durability::Synced, move |tx| {
            let mut other = planned(0, "done");
            other.intended = intended("something else");
            tx.ledger().plan_records(&fence, &[other])
        })
        .await
        .unwrap_err();
    assert_eq!(planned_again.code(), &codes::LEDGER_STATE_UNEXPECTED);
}

// ---- events ----

#[tokio::test]
async fn the_state_rebuilt_from_events_is_the_current_state() {
    let store = memory().await;
    let fence = running(&store, 1, &["a", "b", "c", "d"]).await;
    let proof = evidence(&store, "read").await;

    // a: verified. b: failed in doubt. c: intent, then the lease is lost and
    // it is settled as not landed. d: stays planned.
    lock(&store, fence, 0).await.unwrap();
    let token = intent(&store, fence, 0, "a").await.unwrap();
    store
        .write(Durability::Synced, move |tx| {
            let acted = tx.ledger().mark_acted(token, Acted::default())?;
            let read = ReadBack {
                digest: intended("a"),
                after: None,
                evidence: proof,
                outputs: None,
            };
            tx.ledger().mark_verified(&acted, read)?;
            tx.ledger().append_event(
                &fence,
                a2m_ledger::EventKind::StepStarted,
                &serde_json::json!({ "step": 1 }),
                None,
            )
        })
        .await
        .unwrap();
    lock(&store, fence, 1).await.unwrap();
    let token = intent(&store, fence, 1, "b").await.unwrap();
    store
        .write(Durability::Synced, move |tx| {
            let acted = tx.ledger().mark_acted(token, Acted::default())?;
            let why = stopped_by(FailureClass::Mismatch, "read_back_differs");
            tx.ledger()
                .mark_failed(&fence, acted.record(), &why, Some(proof))
        })
        .await
        .unwrap();
    lock(&store, fence, 2).await.unwrap();
    let _lost = intent(&store, fence, 2, "c").await.unwrap();
    let fence = store
        .write(Durability::Synced, move |tx| {
            let why = stopped_by(FailureClass::PossibleCommit, "runner_lost");
            let next = tx.ledger().new_epoch(fence.run)?;
            tx.ledger()
                .set_state(&next, RunState::Stopped, Some(&why))?;
            tx.ledger().take_lock(&next, record(&next, 2))?;
            tx.ledger().settle(
                &next,
                record(&next, 2),
                a2m_ledger::Reconciled::NotLanded { evidence: proof },
            )?;
            tx.ledger().late_evidence(next.run, proof)?;
            Ok(next)
        })
        .await
        .unwrap();

    let (run, records, events) = store
        .read(move |tx| {
            let mut ledger = tx.ledger();
            Ok((
                ledger.run(fence.run)?,
                ledger.records(fence.run)?,
                ledger.events(fence.run, 0, 10_000)?,
            ))
        })
        .await
        .unwrap();
    // The numbers run from 1 with no gap.
    let numbers: Vec<u64> = events.iter().map(|event| event.seq).collect();
    assert_eq!(numbers, (1..=events.len() as u64).collect::<Vec<_>>());

    let rebuilt = replay(&events).unwrap();
    assert_eq!(rebuilt.state, run.state);
    assert_eq!(rebuilt.epoch, run.epoch);
    assert_eq!(run.attempt, 2);
    assert_eq!(
        run.failure.as_ref().map(|(class, _)| *class),
        Some(FailureClass::PossibleCommit)
    );
    let current: Vec<((u32, u32), RecordState)> = records
        .iter()
        .map(|record| {
            (
                (record.record.step_seq, record.record.record_seq),
                record.state,
            )
        })
        .collect();
    assert_eq!(rebuilt.records.into_iter().collect::<Vec<_>>(), current);
    assert_eq!(
        current.iter().map(|(_, state)| *state).collect::<Vec<_>>(),
        [
            RecordState::Verified,
            RecordState::Failed,
            RecordState::Planned,
            RecordState::Planned
        ]
    );

    // The record that failed in doubt keeps the run from running again, and
    // from being called complete.
    let blocked = store
        .write(Durability::Synced, move |tx| {
            tx.ledger().set_state(&fence, RunState::Running, None)
        })
        .await
        .unwrap_err();
    assert_eq!(blocked.code(), &codes::RECONCILE_REQUIRED);
}

#[tokio::test]
async fn a_read_runs_without_records_and_a_run_completes_only_when_every_record_is_settled() {
    let store = memory().await;
    // A read: queued, validating, running, completed; no approval, no records.
    let read = store
        .write(Durability::Relaxed, move |tx| {
            let mut ledger = tx.ledger();
            let fence = match ledger.open_run(new_run(EffectClass::Read, 1, "read"))? {
                a2m_ledger::Opened::New(fence) => fence,
                a2m_ledger::Opened::Existing(_) => panic!("new"),
            };
            ledger.set_state(&fence, RunState::Validating, None)?;
            let no_records = ledger.plan_records(&fence, &[planned(0, "a")]).unwrap_err();
            assert_eq!(no_records.code(), &codes::LEDGER_STATE_UNEXPECTED);
            ledger.set_state(&fence, RunState::Running, None)?;
            ledger.set_state(&fence, RunState::Completed, None)?;
            ledger.run(fence.run)
        })
        .await
        .unwrap();
    assert_eq!(read.state, RunState::Completed);
    assert_eq!(read.started_at, Some(Timestamp::from_unix_millis(START)));
    assert_eq!(read.finished_at, Some(Timestamp::from_unix_millis(START)));

    // A write run with a record still planned is not complete; a move no
    // rule allows is refused; a run that stops carries its failure.
    let fence = running(&store, 1, &["a"]).await;
    for (to, why) in [
        (RunState::Completed, None),
        (RunState::Queued, None),
        (RunState::NeedsApproval, None),
        (RunState::Stopped, None),
    ] {
        let refused = store
            .write(Durability::Synced, move |tx| {
                tx.ledger().set_state(&fence, to, why)
            })
            .await
            .unwrap_err();
        assert_eq!(refused.code(), &codes::LEDGER_STATE_UNEXPECTED, "{to}");
    }

    // A stopped run stopped again by a later lease takes the new failure,
    // so that it never shows the one before.
    let fence = running(&store, 1, &["b"]).await;
    let first = a2m_core::Failure::new(
        FailureClass::PossibleCommit,
        codes::RUNNER_LOST,
        "The lease ended.",
        "Reconcile.",
    );
    let second = a2m_core::Failure::new(
        FailureClass::Busy,
        codes::STORE_UNAVAILABLE,
        "The store is closed.",
        "Try again.",
    );
    let run = store
        .write(Durability::Synced, move |tx| {
            let mut ledger = tx.ledger();
            ledger.set_state(&fence, RunState::Stopped, Some(&first))?;
            ledger.set_state(&fence, RunState::Stopped, Some(&second))?;
            ledger.run(fence.run)
        })
        .await
        .unwrap();
    assert_eq!(run.state, RunState::Stopped);
    assert_eq!(
        run.failure,
        Some((FailureClass::Busy, codes::STORE_UNAVAILABLE))
    );
}

// ---- blobs ----

#[tokio::test]
async fn a_blob_is_sealed_named_by_a_keyed_digest_and_stored_once() {
    let folder = Folder::new("blobs");
    let store = on_disk(&folder).await;
    let plain = b"customer=Example Ltd; total=12.50".to_vec();
    let blob = |workspace: u8| NewBlob {
        workspace: a2m_core::WorkspaceId::from_bytes([workspace; 16]),
        owner: OwnerRef::service("engine").unwrap(),
        kind: BlobKind::Values,
        class: RetentionClass::Long,
        plain: plain.clone(),
    };
    let name = store.blobs().put(blob(1)).await.unwrap();

    // The name is not the plain hash, and another workspace names the same
    // content differently.
    assert_ne!(name.digest(), Digest::of(&plain));
    let elsewhere = store.blobs().put(blob(2)).await.unwrap();
    assert_ne!(elsewhere, name);

    // The same content again is the same blob: one row, one file.
    assert_eq!(store.blobs().put(blob(1)).await.unwrap(), name);
    let files = || -> Vec<std::path::PathBuf> {
        let mut found = Vec::new();
        for folder in std::fs::read_dir(store.blob_dir().unwrap()).unwrap() {
            for file in std::fs::read_dir(folder.unwrap().path()).unwrap() {
                found.push(file.unwrap().path());
            }
        }
        found
    };
    assert_eq!(files().len(), 2);
    let rows: i64 = folder
        .raw()
        .query_row("SELECT COUNT(*) FROM blobs", [], |row| row.get(0))
        .unwrap();
    assert_eq!(rows, 2);

    // Nothing on disk is in the clear.
    for file in files() {
        let bytes = std::fs::read(&file).unwrap();
        assert!(
            !bytes.windows(7).any(|window| window == b"Example"),
            "{}",
            file.display()
        );
    }
    assert_eq!(
        store.blobs().get(&name).await.unwrap().expose(),
        plain.as_slice()
    );
    assert!(store.blobs().exists(&name).await.unwrap());

    // A name that is not stored, and a row whose file is gone.
    let missing = BlobRef::from_digest(Digest::of(b"nothing"));
    assert_eq!(
        store.blobs().get(&missing).await.unwrap_err().code(),
        &codes::NOT_FOUND
    );
    for file in files() {
        std::fs::remove_file(file).unwrap();
    }
    assert_eq!(
        store.blobs().get(&name).await.unwrap_err().code(),
        &codes::STORE_CORRUPT
    );

    // A store in memory keeps the sealed bytes in the database.
    let store = memory().await;
    let name = store.blobs().put(blob(1)).await.unwrap();
    assert_eq!(
        store.blobs().get(&name).await.unwrap().expose(),
        plain.as_slice()
    );
}

// ---- findings and open questions ----

fn finding(found: &str, level: EvidenceLevel) -> Finding {
    Finding::about(
        Subject::Unknown {
            reason: UnknownReason::ALL[0],
        },
        FindingKind::new("endpoint").unwrap(),
        found,
        Source::LiveSystem,
        ProducedBy {
            provider: "example".to_owned(),
            version: "1.0.0".to_owned(),
            component: None,
        },
        Timestamp::from_unix_millis(START),
    )
    .level(level)
    .coverage(Coverage::Partial)
    .build()
    .unwrap()
}

#[tokio::test]
async fn a_finding_is_stored_once_and_never_above_discovered() {
    let folder = Folder::new("findings");
    let store = on_disk(&folder).await;
    let seen = finding("GET /orders", EvidenceLevel::Discovered);
    let guessed = finding("GET /orders/{id}", EvidenceLevel::Inferred);
    let (one, two) = (seen.clone(), guessed.clone());
    let listed = store
        .write(Durability::Synced, move |tx| {
            let mut findings = tx.findings();
            findings.put(&workspace(), &target(), &one)?;
            findings.put(&workspace(), &target(), &two)?;
            findings.put(&workspace(), &target(), &one)?;
            findings.list(&workspace(), &target())
        })
        .await
        .unwrap();
    assert_eq!(listed.len(), 2);
    let id = seen.id();
    let back = store
        .read(move |tx| tx.findings().get(&workspace(), &target(), &id))
        .await
        .unwrap();
    assert_eq!(back, Some(seen.clone()));

    // The type refuses to make a finding above `discovered`.
    let above = Finding::about(
        Subject::Unknown {
            reason: UnknownReason::ALL[0],
        },
        FindingKind::new("endpoint").unwrap(),
        "x",
        Source::LiveSystem,
        ProducedBy {
            provider: "example".to_owned(),
            version: "1.0.0".to_owned(),
            component: None,
        },
        Timestamp::from_unix_millis(START),
    )
    .level(EvidenceLevel::Approved)
    .build();
    assert!(above.is_err());

    // The table refuses the row, whoever sends it.
    let raw = folder.raw();
    let refused = raw
        .execute(
            "INSERT INTO findings
             SELECT workspace_id, target, 'other', format, 'approved', kind, source, body, observed_at, stored_at
             FROM findings LIMIT 1",
            [],
        )
        .unwrap_err();
    assert!(refused.to_string().contains("CHECK"), "{refused}");
    // A stored finding is never rewritten.
    assert!(raw.execute("UPDATE findings SET body = '{}'", []).is_err());

    // And a body that claims more than its row is refused when it is read.
    raw.execute_batch("DROP TRIGGER findings_never_change")
        .unwrap();
    raw.execute(
        "UPDATE findings SET body = replace(body, '\"discovered\"', '\"approved\"') WHERE id = ?1",
        [id.to_string()],
    )
    .unwrap();
    let raised = store
        .read(move |tx| tx.findings().get(&workspace(), &target(), &id))
        .await
        .unwrap_err();
    assert_eq!(raised.code(), &codes::STORE_CORRUPT);
}

#[tokio::test]
async fn an_open_question_is_settled_by_evidence_and_never_by_a_statement() {
    let store = memory().await;
    let seen = finding("a confirmation dialog", EvidenceLevel::Discovered);
    let stored = seen.clone();
    let id = store
        .write(Durability::Synced, move |tx| {
            let mut findings = tx.findings();
            findings.put(&workspace(), &target(), &stored)?;
            let id = findings.open_question(NewUnknown {
                workspace: workspace(),
                target: target(),
                question: "Does saving a line item ever show a confirmation dialog?".to_owned(),
                scope: "order.line_item_form".to_owned(),
                why_it_matters: "The draft of the line item write depends on it.".to_owned(),
                settled_by_what: "An observation of the save, at `discovered`.".to_owned(),
            })?;
            findings.set_status(id, UnknownStatus::Looking)?;
            Ok(id)
        })
        .await
        .unwrap();
    assert_eq!(id.to_string().parse::<a2m_ledger::UnknownId>().unwrap(), id);

    for settlement in [
        Settlement::default(),
        Settlement {
            findings: vec![Digest::of(b"not stored")],
            evidence: vec![],
        },
        Settlement {
            findings: vec![],
            evidence: vec![BlobRef::from_digest(Digest::of(b"no"))],
        },
    ] {
        let refused = store
            .write(Durability::Synced, move |tx| {
                tx.findings().settle(id, &settlement)
            })
            .await
            .unwrap_err();
        assert_eq!(refused.code(), &codes::LEDGER_STATE_UNEXPECTED);
    }
    let by_status = store
        .write(Durability::Synced, move |tx| {
            tx.findings().set_status(id, UnknownStatus::Settled)
        })
        .await
        .unwrap_err();
    assert_eq!(by_status.code(), &codes::LEDGER_STATE_UNEXPECTED);

    let settlement = Settlement {
        findings: vec![seen.id()],
        evidence: vec![],
    };
    let with = settlement.clone();
    let rows = store
        .write(Durability::Synced, move |tx| {
            tx.findings().settle(id, &with)?;
            tx.findings().unknowns(&workspace(), &target())
        })
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status, UnknownStatus::Settled);
    assert_eq!(rows[0].settlement, Some(settlement));
    let reopened = store
        .write(Durability::Synced, move |tx| {
            tx.findings().set_status(id, UnknownStatus::Open)
        })
        .await
        .unwrap_err();
    assert_eq!(reopened.code(), &codes::LEDGER_STATE_UNEXPECTED);
}

// ---- published versions ----

#[tokio::test]
async fn a_published_plan_is_stored_by_its_hash_and_checked_on_every_load() {
    let folder = Folder::new("versions");
    let store = on_disk(&folder).await;
    let plan = |text: &str| a2m_core::EncodedPlan {
        bytes: text.as_bytes().to_vec(),
        hash: Digest::of(text.as_bytes()),
    };
    let version = |text: &str, number: u32| PublishedVersion {
        workspace: workspace(),
        target: target(),
        version: Version::new(1, number, 0),
        plan: plan(text),
        published_by: principal(1),
        gate_evidence: (number == 1).then(|| r#"{"gates":[]}"#.to_owned()),
    };
    let (first, second) = (plan("plan one").hash, plan("plan two").hash);
    let (one, two) = (version("plan one", 0), version("plan two", 1));
    let listed = store
        .write(Durability::Synced, move |tx| {
            let mut versions = tx.versions();
            versions.put(&one)?;
            versions.put(&two)?;
            versions.put(&one)?;
            versions.make_current(&workspace(), &target(), &first)?;
            versions.make_current(&workspace(), &target(), &second)?;
            assert_eq!(versions.current(&workspace(), &target())?, Some(second));
            // The evidence of its gates is kept with each version as given.
            assert_eq!(
                versions.gate_evidence(&workspace(), &target(), &second)?,
                Some(r#"{"gates":[]}"#.to_owned())
            );
            assert_eq!(
                versions.gate_evidence(&workspace(), &target(), &first)?,
                None
            );
            versions.list(&workspace(), &target())
        })
        .await
        .unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed.iter().filter(|row| row.current).count(), 1);
    // The evidence is never changed, whoever writes to the file (decision
    // 0196, audit finding LE-1).
    let refused = folder
        .raw()
        .execute("UPDATE target_versions SET gate_evidence = '{}'", [])
        .unwrap_err();
    assert!(
        refused.to_string().contains("gate evidence never changes"),
        "{refused}"
    );

    let loaded = store
        .read(move |tx| tx.versions().load(&workspace(), &target(), &first))
        .await
        .unwrap();
    assert_eq!(loaded, plan("plan one"));

    // Bytes that are not the bytes the hash names are refused on the way in.
    let mut wrong = version("plan three", 2);
    wrong.plan.hash = first;
    let refused = store
        .write(Durability::Synced, move |tx| tx.versions().put(&wrong))
        .await
        .unwrap_err();
    assert_eq!(refused.code(), &a2m_core::FailureCode::PLAN_HASH_MISMATCH);

    // A published version never changes; if its bytes are changed behind the
    // store's back, the load says so.
    let raw = folder.raw();
    assert!(
        raw.execute("UPDATE target_versions SET plan = x'00'", [])
            .is_err()
    );
    assert!(raw.execute("DELETE FROM target_versions", []).is_err());
    raw.execute_batch("DROP TRIGGER target_versions_keep_their_plan")
        .unwrap();
    raw.execute(
        "UPDATE target_versions SET plan = x'00' WHERE plan_hash = ?1",
        [first.to_string()],
    )
    .unwrap();
    let damaged = store
        .read(move |tx| tx.versions().load(&workspace(), &target(), &first))
        .await
        .unwrap_err();
    assert_eq!(damaged.code(), &codes::STORE_CORRUPT);
}

// ---- the vault's keys ----

#[tokio::test]
async fn the_vaults_keys_are_kept_as_rows_inserted_once_and_replaced_only_when_unchanged() {
    let folder = Folder::new("keys");
    let store = Store::open(StoreConfig::in_dir(folder.path()))
        .await
        .unwrap();
    let records = store.key_records();
    let record = |id: KeyId, generation: u32, bytes: u8| KeyRecord {
        id,
        layer: Layer::Workspace,
        generation: Generation::new(generation),
        wrapped: Wrapped::from_bytes(vec![bytes; 4]),
        wrapped_by: KeyRef {
            id: KeyId::root(),
            generation: Generation::FIRST,
        },
    };
    let main = KeyId::workspace(&workspace());
    let other = KeyId::workspace(&a2m_core::WorkspaceId::from_bytes([2; 16]));
    assert_eq!(records.newest(&main).await.unwrap(), None);

    records.insert(record(main, 1, 1)).await.unwrap();
    records.insert(record(main, 2, 2)).await.unwrap();
    records.insert(record(other, 1, 3)).await.unwrap();
    let again = records.insert(record(main, 1, 9)).await.unwrap_err();
    assert_eq!(again.code(), &a2m_vault::codes::KEY_RECORD_CONFLICT);
    assert_eq!(
        records.get(&main, Generation::FIRST).await.unwrap(),
        Some(record(main, 1, 1))
    );
    assert_eq!(
        records.newest(&main).await.unwrap(),
        Some(record(main, 2, 2))
    );
    assert_eq!(records.get(&other, Generation::new(2)).await.unwrap(), None);

    let stale = Wrapped::from_bytes(vec![7; 4]);
    let lost = records
        .replace(record(main, 1, 5), &stale)
        .await
        .unwrap_err();
    assert_eq!(lost.code(), &a2m_vault::codes::KEY_RECORD_CONFLICT);
    let current = Wrapped::from_bytes(vec![1; 4]);
    records.replace(record(main, 1, 5), &current).await.unwrap();

    // The rows outlive the store, and the records answer `busy` once it is gone.
    drop(store);
    assert_eq!(
        records.newest(&main).await.unwrap_err().code(),
        &codes::STORE_UNAVAILABLE
    );
    let store = Store::open(StoreConfig::in_dir(folder.path()))
        .await
        .unwrap();
    let records = store.key_records();
    assert_eq!(
        records.get(&main, Generation::FIRST).await.unwrap(),
        Some(record(main, 1, 5))
    );
    assert_eq!(
        records.newest(&other).await.unwrap(),
        Some(record(other, 1, 3))
    );
    // Before a vault is attached, what needs one refuses and health says so.
    assert!(!store.health().await.vault_attached);
    let early = store
        .write(Durability::Synced, |tx| {
            tx.ledger().open_run(new_run(EffectClass::Read, 1, "x"))
        })
        .await
        .unwrap_err();
    assert_eq!(early.code(), &codes::STORE_UNAVAILABLE);
}

// ---- the audit chain ----

#[tokio::test]
async fn a_broken_audit_chain_is_detected() {
    async fn chain(folder: &Folder) -> Store {
        let store = on_disk(folder).await;
        store
            .write(Durability::Synced, |tx| {
                for (number, action) in [
                    "grant.add",
                    "version.publish",
                    "killswitch.set",
                    "policy.publish",
                ]
                .into_iter()
                .enumerate()
                {
                    let seq = tx.audit().append(&AuditEntry {
                        workspace: workspace(),
                        actor: principal(1).to_string(),
                        action: action.to_owned(),
                        subject: "example".to_owned(),
                        detail: "{}".to_owned(),
                    })?;
                    assert_eq!(seq, number as u64 + 1);
                }
                Ok(())
            })
            .await
            .unwrap();
        store
    }
    async fn check(store: &Store) -> ChainCheck {
        store
            .read(|tx| tx.audit().verify(&workspace()))
            .await
            .unwrap()
    }

    let folder = Folder::new("audit");
    let store = chain(&folder).await;
    let ChainCheck::Intact {
        entries: 4,
        head: Some(head),
    } = check(&store).await
    else {
        panic!("the chain is intact");
    };
    let entries = store
        .read(|tx| tx.audit().entries(&workspace(), 0, 10))
        .await
        .unwrap();
    assert_eq!(entries[3].hash, head);
    assert_eq!(entries[1].previous, entries[0].hash);

    // The store refuses a change or a removal.
    let raw = folder.raw();
    assert!(
        raw.execute("UPDATE audit SET actor = 'someone else'", [])
            .is_err()
    );
    assert!(raw.execute("DELETE FROM audit WHERE seq = 2", []).is_err());
    assert_eq!(
        check(&store).await,
        ChainCheck::Intact {
            entries: 4,
            head: Some(head)
        }
    );

    // Someone who can drop the triggers can change the rows; the chain shows it.
    let tamper = |statements: &'static str| async move {
        let folder = Folder::new("audit-tampered");
        let store = chain(&folder).await;
        folder
            .raw()
            .execute_batch(&format!(
                "DROP TRIGGER audit_never_changes; DROP TRIGGER audit_is_kept; {statements}"
            ))
            .unwrap();
        check(&store).await
    };
    // Changed.
    assert_eq!(
        tamper("UPDATE audit SET subject = 'another' WHERE seq = 2").await,
        ChainCheck::Broken { at: 2 }
    );
    // Removed.
    assert_eq!(
        tamper("DELETE FROM audit WHERE seq = 3").await,
        ChainCheck::Broken { at: 3 }
    );
    // Removed, with the later entries renumbered to hide it.
    assert_eq!(
        tamper("DELETE FROM audit WHERE seq = 2; UPDATE audit SET seq = seq - 1 WHERE seq > 2")
            .await,
        ChainCheck::Broken { at: 2 }
    );
    // Reordered.
    assert_eq!(
        tamper(
            "UPDATE audit SET seq = 9 WHERE seq = 2; UPDATE audit SET seq = 2 WHERE seq = 3;
             UPDATE audit SET seq = 3 WHERE seq = 9"
        )
        .await,
        ChainCheck::Broken { at: 2 }
    );
}

/// The store fails closed on its own: a second open of one data folder
/// is refused by name while the first is open, even when no engine lock
/// stands in the way, and is let in when the first has closed.
#[tokio::test]
async fn a_second_open_of_one_data_folder_is_refused_while_the_first_is_open() {
    let folder = Folder::new("twice");
    let first = Store::open(StoreConfig::in_dir(folder.path()))
        .await
        .unwrap();

    let second = Store::open(StoreConfig::in_dir(folder.path()))
        .await
        .unwrap_err();
    assert_eq!(second.class(), FailureClass::Busy);
    assert_eq!(second.code(), &codes::STORE_UNAVAILABLE);

    // A clone is the same store: the folder is still held.
    let clone = first.clone();
    drop(first);
    let still = Store::open(StoreConfig::in_dir(folder.path()))
        .await
        .unwrap_err();
    assert_eq!(still.code(), &codes::STORE_UNAVAILABLE);

    // Let go on purpose, the folder opens though a clone is still held.
    clone.release_folder();
    let released = Store::open(StoreConfig::in_dir(folder.path())).await;
    assert!(released.is_ok(), "{:?}", released.err());
    drop(released);

    drop(clone);
    let again = Store::open(StoreConfig::in_dir(folder.path())).await;
    assert!(again.is_ok(), "{:?}", again.err());
}
