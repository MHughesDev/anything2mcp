//! A real store, for real intent tokens: the handle refuses a committing
//! step without the token the ledger gave for the record's stored intent.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use a2m_core::{
    Digest, EffectClass, Environment, Failure, OperationName, PrincipalId, RecordKey, RunState,
    TargetName, Timestamp, Value, WorkspaceId,
};
use a2m_ledger::{
    Durability, Fence, IntentToken, NewRun, Opened, PlannedRecord, RecordIntent, RecordRef,
    RetentionClass, Store, StoreConfig, ToolKind,
};
use a2m_logs::ManualClock;
use a2m_vault::{MemoryKeyStore, RandomSource, Vault};

struct Counted(AtomicU64);

impl RandomSource for Counted {
    fn fill(&self, out: &mut [u8]) -> Result<(), Failure> {
        for chunk in out.chunks_mut(Digest::LENGTH) {
            let number = self.0.fetch_add(1, Ordering::SeqCst);
            let block = Digest::of(&number.to_le_bytes());
            chunk.copy_from_slice(&block.as_bytes()[..chunk.len()]);
        }
        Ok(())
    }
}

fn counted() -> Arc<Counted> {
    Arc::new(Counted(AtomicU64::new(0)))
}

pub async fn store() -> Store {
    let config = StoreConfig::in_memory()
        .with_clock(Arc::new(ManualClock::starting_at(
            Timestamp::from_unix_millis(1_800_000_000_000),
        )))
        .with_random(counted());
    let store = Store::open(config).await.unwrap();
    let vault = Arc::new(Vault::with_random(
        Arc::new(MemoryKeyStore::with_random(counted())),
        Arc::new(store.key_records()),
        counted(),
    ));
    store.attach(vault).await.unwrap();
    store
}

pub fn key(name: &str) -> RecordKey {
    RecordKey::new(vec![Value::from(name)]).unwrap()
}

fn intended(name: &str) -> Digest {
    Digest::of(format!("intended {name}").as_bytes())
}

pub fn record(fence: &Fence, record_seq: u32) -> RecordRef {
    RecordRef {
        run: fence.run,
        step_seq: 0,
        record_seq,
    }
}

/// A write run with the named records planned, approved, and running.
pub async fn running(store: &Store, names: &[&str]) -> Fence {
    let names: Vec<String> = names.iter().map(|name| (*name).to_owned()).collect();
    let workspace = WorkspaceId::from_bytes([1; 16]);
    store
        .write(Durability::Synced, move |tx| {
            let diff = Digest::of(b"diff");
            let fence = {
                let mut ledger = tx.ledger();
                let Opened::New(opened) = ledger.open_run(NewRun {
                    workspace,
                    principal: PrincipalId::from_bytes([1; 16]),
                    client_label: "test".to_owned(),
                    tool_name: OperationName::new("example_item_set_quantity").unwrap(),
                    tool_kind: ToolKind::Operation,
                    target: TargetName::new("example").unwrap(),
                    target_version: Digest::of(b"plan"),
                    environment: Environment::new("test").unwrap(),
                    identity: None,
                    effect: EffectClass::Write,
                    input_hash: Digest::of(names.join(",").as_bytes()),
                    input: None,
                    retention: RetentionClass::Long,
                })?
                else {
                    panic!("the run exists already");
                };
                let fence = ledger.new_epoch(opened.run)?;
                ledger.set_state(&fence, RunState::Validating, None)?;
                let records: Vec<_> = names
                    .iter()
                    .enumerate()
                    .map(|(at, name)| PlannedRecord {
                        step_seq: 0,
                        record_seq: u32::try_from(at).unwrap(),
                        key: key(name),
                        operation: OperationName::new("example_item_set_quantity").unwrap(),
                        intended: intended(name),
                        intended_values: None,
                        before: None,
                        unchanged: false,
                    })
                    .collect();
                ledger.plan_records(&fence, &records)?;
                ledger.store_diff(&fence, diff, None)?;
                ledger.set_state(&fence, RunState::NeedsApproval, None)?;
                fence
            };
            // The policy engine's stand-in: an approval, used once.
            let clearance = a2m_ledger::testing::approve(tx, &fence)?;
            tx.ledger().begin_writes(&fence, clearance)?;
            Ok(fence)
        })
        .await
        .unwrap()
}

/// The token of one planned record's stored intent.
pub async fn token(store: &Store, fence: Fence, record_seq: u32, name: &str) -> IntentToken {
    store
        .write(Durability::Synced, move |tx| {
            tx.ledger()
                .take_lock(&fence, record(&fence, record_seq))
                .map(|_| ())
        })
        .await
        .unwrap();
    let intents = vec![RecordIntent {
        record: record(&fence, record_seq),
        intended: intended(name),
        mode: None,
    }];
    store
        .store_intents(&fence, intents)
        .await
        .unwrap()
        .remove(0)
}
