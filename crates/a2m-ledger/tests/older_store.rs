//! Package 9.2a (decision 0397): a store written by the version before
//! opens with the code of today, migrates, and reads its runs.
//!
//! The fixture under `tests/fixtures/store-0028/` is a data folder's store
//! as the build of migration 0028 wrote it: one write run with a record
//! verified, one mid-write and one still planned, two evidence blobs, and
//! an audit chain of two entries. Every value in it is made up by these
//! tests. It is kept as it was written and never made again: from the next
//! migration on, the test opens a store of the schema before, which is
//! what it is for. When a release is cut, the store of that release may be
//! added beside it, made by `the_fixture_is_made_by_this_build`, and kept
//! as well.
//!
//! The vault's root is derived from a made-up passphrase and the salt kept
//! in the fixture, as on a machine with no key store of its own, so the same
//! root opens the fixture's key records on every machine.

#![allow(
    clippy::unwrap_used,
    reason = "this file holds only tests; clippy.toml allows unwrap in tests, which it tells by the #[test] attribute, and the helpers the tests share do not carry it"
)]

mod common;

use std::path::Path;
use std::sync::Arc;

use a2m_core::{RecordState, RunState, Secret, Timestamp};
use a2m_ledger::{
    Acted, AuditEntry, ChainCheck, DATABASE_FILE_NAME, Durability, ReadBack, RunQuery, migrations,
};
use a2m_telemetry::ManualClock;
use a2m_vault::{KeyStore, OsRandom, PassphraseKeyStore, PassphraseRoute, PassphraseSalt};
use common::*;

/// The made-up passphrase the fixture's root is derived from.
const PASSPHRASE: &str = "example-store-fixture-passphrase";

/// The file in the fixture that keeps the salt of the derivation.
const SALT_FILE: &str = "vault.salt";

/// The store in `folder`, with the root derived from the passphrase and the
/// salt kept there, which is made when it is missing.
async fn open_kept(folder: &Path) -> (a2m_ledger::Store, Arc<a2m_vault::Vault>) {
    let salt = PassphraseSalt::load_or_create(&folder.join(SALT_FILE), &OsRandom)
        .await
        .unwrap();
    let keys =
        PassphraseKeyStore::derive(PassphraseRoute::Enabled, &Secret::new(PASSPHRASE), &salt)
            .await
            .unwrap();
    let clock = Arc::new(ManualClock::starting_at(Timestamp::from_unix_millis(START)));
    open_keyed(Some(folder), Arc::new(keys) as Arc<dyn KeyStore>, clock).await
}

/// The folder of the fixture: the store of a data folder, as written.
fn fixture() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("store-0028")
}

/// Copies a folder and what it holds, so that the test never opens the
/// fixture in place: opening a store writes to it.
fn copy_folder(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap().path();
        let target = to.join(entry.file_name().unwrap());
        if entry.is_dir() {
            copy_folder(&entry, &target);
        } else {
            std::fs::copy(&entry, &target).unwrap();
        }
    }
}

/// The state the fixture holds, written into the store in `folder`.
async fn write_the_fixture(folder: &Path) {
    let (store, vault) = open_kept(folder).await;
    let fence = running(&store, 1, &["done", "mid", "todo"]).await;
    let proof = evidence(&store, "the values read back").await;
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
            tx.ledger().mark_verified(
                &acted,
                ReadBack {
                    digest: intended("done"),
                    after: Some(proof),
                    evidence: proof,
                    outputs: None,
                },
            )
        })
        .await
        .unwrap();
    lock(&store, fence, 1).await.unwrap();
    drop(intent(&store, fence, 1, "mid").await.unwrap());
    store
        .write(Durability::Synced, |tx| {
            for action in ["version.publish", "grant.add"] {
                tx.audit().append(&AuditEntry {
                    workspace: workspace(),
                    actor: principal(1).to_string(),
                    action: action.to_owned(),
                    subject: "example".to_owned(),
                    detail: "{}".to_owned(),
                })?;
            }
            Ok(())
        })
        .await
        .unwrap();
    drop(vault);
    store.release_folder();
    drop(store);
    // The lock files are the running engine's, not a part of what it keeps.
    let _ = std::fs::remove_file(folder.join(a2m_ledger::STORE_LOCK_NAME));
    let _ = std::fs::remove_file(folder.join(format!("{SALT_FILE}.lock")));
}

/// Makes the fixture of this build in the folder `A2M_STORE_FIXTURE_OUT`
/// names. Run by hand, once, when a release's store is to be kept; never to
/// replace a fixture that is committed.
#[tokio::test]
#[ignore = "run by hand to keep the store of a release as a fixture"]
async fn the_fixture_is_made_by_this_build() {
    let out = std::env::var("A2M_STORE_FIXTURE_OUT").unwrap();
    let out = Path::new(&out);
    assert!(
        !out.join(DATABASE_FILE_NAME).exists(),
        "a kept fixture is never written over"
    );
    write_the_fixture(out).await;
}

#[tokio::test]
async fn a_store_written_by_the_version_before_opens_migrates_and_reads_its_runs() {
    let folder = Folder::new("older");
    copy_folder(&fixture(), folder.path());
    let kept: Vec<i64> = {
        let raw = folder.raw();
        let mut statement = raw
            .prepare("SELECT number FROM schema_migrations ORDER BY number")
            .unwrap();
        let rows = statement.query_map([], |row| row.get(0)).unwrap();
        rows.map(Result::unwrap).collect()
    };
    // The fixture is of the schema it was made with: every migration up to
    // 0028 that this build carries, and no later one.
    let carried: Vec<i64> = migrations()
        .unwrap()
        .iter()
        .map(|migration| i64::from(migration.number))
        .collect();
    let then: Vec<i64> = carried.iter().copied().filter(|n| *n <= 28).collect();
    assert_eq!(kept, then);

    let (store, _vault) = open_kept(folder.path()).await;

    // Every migration this build carries is applied, those it kept first.
    let applied: Vec<i64> = {
        let raw = folder.raw();
        let mut statement = raw
            .prepare("SELECT number FROM schema_migrations ORDER BY number")
            .unwrap();
        let rows = statement.query_map([], |row| row.get(0)).unwrap();
        rows.map(Result::unwrap).collect()
    };
    assert_eq!(applied, carried);

    // The run is found by its caller, as it was left.
    let query = RunQuery {
        workspace: workspace(),
        principal: principal(1),
        states: Vec::new(),
        tool: None,
        target: None,
        since: None,
        caller_key: None,
        after: None,
        limit: 10,
    };
    let runs = store
        .read(move |tx| tx.ledger().runs(&query))
        .await
        .unwrap();
    assert_eq!(runs.len(), 1);
    let run = runs[0].clone();
    assert_eq!(run.state, RunState::Running);
    assert_eq!(run.diff_hash, Some(diff_hash()));
    assert!(run.approval.is_some());

    let id = run.id;
    let records = store.read(move |tx| tx.ledger().records(id)).await.unwrap();
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

    // The record left mid-write is found for reconcile, and the evidence
    // still opens with the vault's keys the store kept.
    let doubtful = store
        .read(move |tx| tx.ledger().in_doubt(id))
        .await
        .unwrap();
    assert_eq!(doubtful.len(), 1);
    assert_eq!(doubtful[0].key, key("mid"));
    let proof = records[0].evidence.unwrap();
    let opened = store.blobs().get(&proof).await.unwrap();
    assert_eq!(opened.expose(), b"the values read back");

    // The audit chain is whole.
    let chain = store
        .read(|tx| tx.audit().verify(&workspace()))
        .await
        .unwrap();
    assert!(
        matches!(chain, ChainCheck::Intact { entries: 2, .. }),
        "{chain:?}"
    );
}
