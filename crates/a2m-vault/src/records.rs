//! Where wrapped workspace and owner keys are kept: the `KeyRecords`
//! interface, and an in-memory implementation for tests.
//!
//! The vault sits below the store, so it defines this interface and
//! `a2m-ledger` implements it over a table of its own.

use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};

use a2m_core::Failure;

use crate::failure;
use crate::material::Wrapped;
use crate::names::{Generation, KeyId, KeyRef, Layer};
use crate::store::BoxFuture;

/// One generation of one wrapped key. It holds ciphertext and names, and
/// nothing in the clear.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyRecord {
    /// Which key this is.
    pub id: KeyId,
    /// Its layer: `workspace`, `owner`, or `digest`.
    pub layer: Layer,
    /// Which generation of the key this is.
    pub generation: Generation,
    /// The key, wrapped by the key above it.
    pub wrapped: Wrapped,
    /// The key above it, and which generation of that key wrapped this one.
    /// For a workspace key and for the digest key this is the root, whose
    /// own version travels inside `wrapped`.
    pub wrapped_by: KeyRef,
}

/// Where wrapped keys persist. One row per key and generation.
///
/// A record is inserted once, and replaced only when the key above it is
/// rotated and this key is wrapped again. A generation is never removed
/// while a sealed item may still name it.
pub trait KeyRecords: Send + Sync {
    /// The record of `id` at `generation`, or `None`.
    fn get<'a>(
        &'a self,
        id: &'a KeyId,
        generation: Generation,
    ) -> BoxFuture<'a, Result<Option<KeyRecord>, Failure>>;

    /// The record of `id` with the highest generation, or `None` when the
    /// key has never been made.
    fn newest<'a>(&'a self, id: &'a KeyId) -> BoxFuture<'a, Result<Option<KeyRecord>, Failure>>;

    /// Insert only. Refuses with `busy`/`key_record_conflict` when a record
    /// with this id and generation exists: a key is never overwritten by
    /// accident.
    fn insert(&self, record: KeyRecord) -> BoxFuture<'_, Result<(), Failure>>;

    /// For rotation of the key above: stores `record` in place of the record
    /// with its id and generation, only if that record's wrapped bytes are
    /// still `expected`. Otherwise `busy`/`key_record_conflict`, and nothing
    /// changes.
    fn replace<'a>(
        &'a self,
        record: KeyRecord,
        expected: &'a Wrapped,
    ) -> BoxFuture<'a, Result<(), Failure>>;
}

/// The failure an implementation of [`KeyRecords`] returns when a record
/// exists already or changed before it could be replaced.
#[must_use]
pub fn key_record_conflict() -> Failure {
    failure::record_conflict()
}

/// Key records held in memory. For tests, where it stands in for the store's
/// table: keep the same `MemoryKeyRecords` across a simulated restart.
#[derive(Debug, Default)]
pub struct MemoryKeyRecords {
    rows: Mutex<BTreeMap<(KeyId, Generation), KeyRecord>>,
}

impl MemoryKeyRecords {
    /// An empty set of records.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Every record, in order of key and generation. For tests that ask what
    /// was stored.
    #[must_use]
    pub fn all(&self) -> Vec<KeyRecord> {
        self.rows().values().cloned().collect()
    }

    fn rows(&self) -> std::sync::MutexGuard<'_, BTreeMap<(KeyId, Generation), KeyRecord>> {
        self.rows.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl KeyRecords for MemoryKeyRecords {
    fn get<'a>(
        &'a self,
        id: &'a KeyId,
        generation: Generation,
    ) -> BoxFuture<'a, Result<Option<KeyRecord>, Failure>> {
        Box::pin(async move { Ok(self.rows().get(&(*id, generation)).cloned()) })
    }

    fn newest<'a>(&'a self, id: &'a KeyId) -> BoxFuture<'a, Result<Option<KeyRecord>, Failure>> {
        Box::pin(async move {
            let first = (*id, Generation::new(0));
            let last = (*id, Generation::new(u32::MAX));
            Ok(self
                .rows()
                .range(first..=last)
                .next_back()
                .map(|(_, record)| record.clone()))
        })
    }

    fn insert(&self, record: KeyRecord) -> BoxFuture<'_, Result<(), Failure>> {
        Box::pin(async move {
            let mut rows = self.rows();
            let place = (record.id, record.generation);
            if rows.contains_key(&place) {
                return Err(key_record_conflict());
            }
            rows.insert(place, record);
            Ok(())
        })
    }

    fn replace<'a>(
        &'a self,
        record: KeyRecord,
        expected: &'a Wrapped,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async move {
            let mut rows = self.rows();
            match rows.get_mut(&(record.id, record.generation)) {
                Some(row) if row.wrapped == *expected => {
                    *row = record;
                    Ok(())
                }
                _ => Err(key_record_conflict()),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a2m_core::WorkspaceId;

    fn record(id: KeyId, generation: u32, bytes: u8) -> KeyRecord {
        KeyRecord {
            id,
            layer: Layer::Workspace,
            generation: Generation::new(generation),
            wrapped: Wrapped::from_bytes(vec![bytes; 4]),
            wrapped_by: KeyRef {
                id: KeyId::root(),
                generation: Generation::FIRST,
            },
        }
    }

    #[tokio::test]
    async fn a_record_is_inserted_once_and_replaced_only_when_unchanged() {
        let records = MemoryKeyRecords::new();
        let main = KeyId::workspace(&WorkspaceId::from_bytes([1; 16]));
        let other = KeyId::workspace(&WorkspaceId::from_bytes([2; 16]));
        assert_eq!(records.newest(&main).await.unwrap(), None);

        records.insert(record(main, 1, 1)).await.unwrap();
        records.insert(record(main, 2, 2)).await.unwrap();
        records.insert(record(other, 1, 3)).await.unwrap();
        let again = records.insert(record(main, 1, 9)).await.unwrap_err();
        assert_eq!(again.code(), &failure::KEY_RECORD_CONFLICT);
        assert_eq!(again.class(), a2m_core::FailureClass::Busy);

        let first = Generation::FIRST;
        assert_eq!(
            records.get(&main, first).await.unwrap(),
            Some(record(main, 1, 1))
        );
        assert_eq!(
            records.newest(&main).await.unwrap(),
            Some(record(main, 2, 2))
        );
        assert_eq!(
            records.newest(&other).await.unwrap(),
            Some(record(other, 1, 3))
        );
        assert_eq!(records.get(&other, Generation::new(2)).await.unwrap(), None);

        let stale = Wrapped::from_bytes(vec![7; 4]);
        let lost = records
            .replace(record(main, 1, 5), &stale)
            .await
            .unwrap_err();
        assert_eq!(lost.code(), &failure::KEY_RECORD_CONFLICT);
        assert_eq!(
            records.get(&main, first).await.unwrap(),
            Some(record(main, 1, 1))
        );

        let current = Wrapped::from_bytes(vec![1; 4]);
        records.replace(record(main, 1, 5), &current).await.unwrap();
        assert_eq!(
            records.get(&main, first).await.unwrap(),
            Some(record(main, 1, 5))
        );
        assert_eq!(records.all().len(), 3);
    }
}
