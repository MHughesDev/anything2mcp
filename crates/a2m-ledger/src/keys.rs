//! The vault's wrapped keys, kept as rows: the store's side of the vault's
//! `KeyRecords`.
//!
//! A row holds what the vault handed over, already wrapped. The store never
//! unwraps, chooses, or rotates a key.

use std::sync::Weak;

use a2m_core::Failure;
use a2m_vault::{BoxFuture, Generation, KeyId, KeyRecord, KeyRecords, KeyRef, Layer, Wrapped};

use crate::codec::{parse, small};
use crate::store::{Inner, upgrade};
use crate::tx::{Durability, Row, SqlValue};

/// Where the vault's wrapped keys persist: the store's table of them. Made
/// by [`Store::key_records`](crate::Store::key_records). It holds the store
/// weakly, so the vault does not keep a closed store open; after the store
/// is dropped every call is `busy`/`store_unavailable`.
pub struct StoreKeyRecords {
    store: Weak<Inner>,
}

impl StoreKeyRecords {
    pub(crate) fn new(store: Weak<Inner>) -> Self {
        Self { store }
    }
}

impl std::fmt::Debug for StoreKeyRecords {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("StoreKeyRecords")
    }
}

fn record(row: &Row) -> Result<KeyRecord, Failure> {
    Ok(KeyRecord {
        id: parse::<KeyId>(row.text(0)?)?,
        generation: Generation::new(small(row.int(1)?)?),
        layer: parse::<Layer>(row.text(2)?)?,
        wrapped: Wrapped::from_bytes(row.bytes(3)?.to_vec()),
        wrapped_by: KeyRef {
            id: parse::<KeyId>(row.text(4)?)?,
            generation: Generation::new(small(row.int(5)?)?),
        },
    })
}

fn values(record: &KeyRecord) -> Vec<SqlValue> {
    vec![
        SqlValue::from(record.id.to_string()),
        SqlValue::from(record.generation.get()),
        SqlValue::from(record.layer.as_str()),
        SqlValue::from(record.wrapped.as_bytes().to_vec()),
        SqlValue::from(record.wrapped_by.id.to_string()),
        SqlValue::from(record.wrapped_by.generation.get()),
    ]
}

impl KeyRecords for StoreKeyRecords {
    fn get<'a>(
        &'a self,
        id: &'a KeyId,
        generation: Generation,
    ) -> BoxFuture<'a, Result<Option<KeyRecord>, Failure>> {
        let params = vec![SqlValue::from(id.to_string()), SqlValue::from(generation.get())];
        Box::pin(async move {
            // Read on the writing thread: a key written a moment ago must be
            // seen, and a store in memory has no other connection.
            upgrade(&self.store)?
                .write(Durability::Relaxed, move |tx| {
                    tx.row(
                        "SELECT key_id, generation, layer, wrapped, wrapped_by_id, wrapped_by_generation
                         FROM vault_keys WHERE key_id = ?1 AND generation = ?2",
                        &params,
                    )?
                    .map(|row| record(&row))
                    .transpose()
                })
                .await
        })
    }

    fn newest<'a>(&'a self, id: &'a KeyId) -> BoxFuture<'a, Result<Option<KeyRecord>, Failure>> {
        let params = vec![SqlValue::from(id.to_string())];
        Box::pin(async move {
            upgrade(&self.store)?
                .write(Durability::Relaxed, move |tx| {
                    tx.row(
                        "SELECT key_id, generation, layer, wrapped, wrapped_by_id, wrapped_by_generation
                         FROM vault_keys WHERE key_id = ?1 ORDER BY generation DESC LIMIT 1",
                        &params,
                    )?
                    .map(|row| record(&row))
                    .transpose()
                })
                .await
        })
    }

    fn insert(&self, record: KeyRecord) -> BoxFuture<'_, Result<(), Failure>> {
        Box::pin(async move {
            upgrade(&self.store)?
                .write(Durability::Synced, move |tx| {
                    let inserted = tx.exec(
                        "INSERT INTO vault_keys
                            (key_id, generation, layer, wrapped, wrapped_by_id, wrapped_by_generation)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                         ON CONFLICT (key_id, generation) DO NOTHING",
                        &values(&record),
                    )?;
                    if inserted == 1 {
                        Ok(())
                    } else {
                        Err(a2m_vault::key_record_conflict())
                    }
                })
                .await
        })
    }

    fn replace<'a>(
        &'a self,
        record: KeyRecord,
        expected: &'a Wrapped,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        let expected = expected.as_bytes().to_vec();
        Box::pin(async move {
            upgrade(&self.store)?
                .write(Durability::Synced, move |tx| {
                    let mut params = values(&record);
                    params.push(SqlValue::from(expected));
                    let replaced = tx.exec(
                        "UPDATE vault_keys
                         SET layer = ?3, wrapped = ?4, wrapped_by_id = ?5, wrapped_by_generation = ?6
                         WHERE key_id = ?1 AND generation = ?2 AND wrapped = ?7",
                        &params,
                    )?;
                    if replaced == 1 {
                        Ok(())
                    } else {
                        Err(a2m_vault::key_record_conflict())
                    }
                })
                .await
        })
    }
}
