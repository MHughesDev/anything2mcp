//! The vault: sealing and opening under the four key layers.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use a2m_core::{Failure, Secret, WorkspaceId};

use crate::cipher::{self, BoundTo};
use crate::digest::VaultDigest;
use crate::failure;
use crate::material::{KeyMaterial, Opened, OsRandom, RandomSource};
use crate::names::{Binding, Generation, KeyId, KeyRef, Layer, SealContext};
use crate::records::{KeyRecord, KeyRecords};
use crate::redactor::{Redactor, RedactorHealth};
use crate::sealed::{self, Sealed};
use crate::store::{KeyStore, StoreDescription, StoreHealth};

const ITEM: &str = "a2m-vault/item/1";
const DATA_KEY: &str = "a2m-vault/data-key/1";
const UPPER_KEY: &str = "a2m-vault/upper-key/1";

/// Seals and opens. It keeps nothing itself: `seal` returns a [`Sealed`]
/// value and the caller stores it; wrapped workspace and owner keys go
/// through [`KeyRecords`]; the root never leaves its [`KeyStore`].
///
/// The vault does not decide who may open an item. An item sealed for one
/// owner opens only under that owner's context, by cryptography; whether the
/// caller really acts for that owner is decided before the call, by the
/// session keeper and the scheduler.
pub struct Vault {
    store: Arc<dyn KeyStore>,
    records: Arc<dyn KeyRecords>,
    random: Arc<dyn RandomSource>,
    redactor: Arc<Redactor>,
    /// Unwrapped workspace and owner keys and the digest key, in wiped memory.
    keys: Mutex<BTreeMap<KeyRef, Arc<KeyMaterial>>>,
    /// The newest generation of each key this process has seen.
    newest: Mutex<BTreeMap<KeyId, Generation>>,
}

/// The state of the vault, for the kernel's health state and for `doctor`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultHealth {
    /// Which key store holds the root, and how strong it is.
    pub store: StoreDescription,
    /// Whether that store answers and holds a root. `EntryMissing` is what a
    /// new installation reports until its first item is sealed.
    pub store_health: StoreHealth,
    /// The redactor's registered set against its bound.
    pub redactor: RedactorHealth,
}

impl Vault {
    /// A vault over a root back end and the place key records are kept.
    /// Nothing is read until the first item is sealed or opened.
    #[must_use]
    pub fn new(store: Arc<dyn KeyStore>, records: Arc<dyn KeyRecords>) -> Self {
        Self::with_random(store, records, Arc::new(OsRandom))
    }

    /// The same, taking keys and nonces from `random`. For tests that need
    /// what they seal to be the same on every run.
    #[must_use]
    pub fn with_random(
        store: Arc<dyn KeyStore>,
        records: Arc<dyn KeyRecords>,
        random: Arc<dyn RandomSource>,
    ) -> Self {
        Self {
            store,
            records,
            random,
            redactor: Arc::new(Redactor::new()),
            keys: Mutex::new(BTreeMap::new()),
            newest: Mutex::new(BTreeMap::new()),
        }
    }

    /// Seals `plain` as the item `cx` describes. Each call makes a fresh
    /// data key and a fresh nonce, so two seals of the same bytes differ, and
    /// an item that is rewritten has its data key rotated.
    ///
    /// # Errors
    ///
    /// `busy`/`key_store_unreachable` or `busy`/`key_store_entry_missing`
    /// when the keys above the item cannot be had; whatever the key records
    /// return; `busy`/`randomness_unavailable`.
    pub async fn seal(&self, cx: &SealContext, plain: &[u8]) -> Result<Sealed, Failure> {
        let owner_id = KeyId::owner(&cx.workspace, &cx.owner);
        let (owner_key, owner) = self.newest_owner_key(&cx.workspace, owner_id).await?;
        let data = KeyMaterial::generate(self.random.as_ref())?;
        let nonce = cipher::fresh_nonce(self.random.as_ref())?;
        let ciphertext = cipher::encrypt(
            &data,
            &nonce,
            sealed::bound_to(ITEM, cx, owner_key).as_bytes(),
            plain,
        )?;
        let data_key = cipher::wrap(
            &owner,
            &data,
            sealed::bound_to(DATA_KEY, cx, owner_key).as_bytes(),
            self.random.as_ref(),
        )?;
        Ok(Sealed::new(owner_key, data_key, nonce, ciphertext))
    }

    /// Opens bytes that are not secrets: evidence, ledger values. The caller
    /// keeps them out of logs.
    ///
    /// # Errors
    ///
    /// `input`/`sealed_item_mismatch` when the envelope does not open under
    /// `cx`: another workspace, owner, kind, or item, or a damaged envelope.
    /// The failure never says which. `busy`/`key_store_unreachable` or
    /// `busy`/`key_store_entry_missing` when the keys above cannot be had.
    pub async fn open(&self, cx: &SealContext, sealed: &Sealed) -> Result<Opened, Failure> {
        let owner_key = sealed.owner_key();
        if owner_key.id != KeyId::owner(&cx.workspace, &cx.owner) {
            return Err(failure::mismatch());
        }
        let owner = self
            .owner_key(&cx.workspace, owner_key)
            .await?
            .ok_or_else(failure::mismatch)?;
        let data = cipher::unwrap(
            &owner,
            sealed.data_key().as_bytes(),
            sealed::bound_to(DATA_KEY, cx, owner_key).as_bytes(),
        )?;
        let plain = cipher::decrypt(
            &data,
            sealed.nonce(),
            sealed::bound_to(ITEM, cx, owner_key).as_bytes(),
            sealed.ciphertext(),
        )?;
        Ok(Opened::new(plain))
    }

    /// Opens a secret. The value is registered with the redactor before this
    /// returns, so it is masked wherever it later appears in text.
    ///
    /// # Errors
    ///
    /// As [`Vault::open`]; also `input`/`sealed_item_mismatch` when the item
    /// is not text, which a secret always is.
    pub async fn open_secret(&self, cx: &SealContext, sealed: &Sealed) -> Result<Secret, Failure> {
        let text = self
            .open(cx, sealed)
            .await?
            .into_text()
            .ok_or_else(failure::mismatch)?;
        let secret = Secret::new(text);
        self.redactor.register(&secret);
        Ok(secret)
    }

    /// Registers a secret that reached the runtime some other way, such as a
    /// token a sign-in returned. The session keeper calls it before the
    /// secret goes anywhere else.
    pub fn register(&self, secret: &Secret) {
        self.redactor.register(secret);
    }

    /// The redactor, handed to everything that emits text.
    #[must_use]
    pub fn redactor(&self) -> Arc<Redactor> {
        Arc::clone(&self.redactor)
    }

    /// The keyed digest: names for blobs and stand-ins for restricted
    /// values that reveal nothing to someone who guesses. The installation's
    /// digest key is made at first use and kept, wrapped by the root, beside
    /// the workspace keys.
    ///
    /// # Errors
    ///
    /// As [`Vault::seal`].
    pub async fn keyed_digest(&self) -> Result<Arc<VaultDigest>, Failure> {
        let (_, key) = self
            .newest_under_root(KeyId::digest(), Layer::Digest)
            .await?;
        Ok(Arc::new(VaultDigest::new(&key)))
    }

    /// The state of the key store and of the redactor.
    pub async fn health(&self) -> VaultHealth {
        VaultHealth {
            store: self.store.describe(),
            store_health: self.store.health().await,
            redactor: self.redactor.health(),
        }
    }

    fn cached(&self, key: KeyRef) -> Option<Arc<KeyMaterial>> {
        self.keys
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&key)
            .cloned()
    }

    fn cache(&self, key: KeyRef, material: KeyMaterial) -> Arc<KeyMaterial> {
        let mut keys = self.keys.lock().unwrap_or_else(PoisonError::into_inner);
        Arc::clone(keys.entry(key).or_insert_with(|| Arc::new(material)))
    }

    fn newest_seen(&self, id: &KeyId) -> Option<Generation> {
        self.newest
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(id)
            .copied()
    }

    fn saw_newest(&self, key: KeyRef) {
        let mut newest = self.newest.lock().unwrap_or_else(PoisonError::into_inner);
        let seen = newest.entry(key.id).or_insert(key.generation);
        *seen = (*seen).max(key.generation);
    }

    /// A key the root wraps, a workspace's or the digest key, at one
    /// generation; `None` when no record of it exists.
    async fn under_root(
        &self,
        key: KeyRef,
        layer: Layer,
        record: Option<KeyRecord>,
    ) -> Result<Option<Arc<KeyMaterial>>, Failure> {
        if let Some(material) = self.cached(key) {
            return Ok(Some(material));
        }
        let record = match record {
            Some(record) => record,
            None => match self.records.get(&key.id, key.generation).await? {
                Some(record) => record,
                None => return Ok(None),
            },
        };
        let material = self
            .store
            .unwrap(&record.wrapped, &root_binding(key, layer))
            .await?;
        Ok(Some(self.cache(key, material)))
    }

    /// The newest generation of a key the root wraps, made and recorded when
    /// there is none.
    async fn newest_under_root(
        &self,
        id: KeyId,
        layer: Layer,
    ) -> Result<(KeyRef, Arc<KeyMaterial>), Failure> {
        if let Some(generation) = self.newest_seen(&id) {
            let key = KeyRef { id, generation };
            if let Some(material) = self.cached(key) {
                return Ok((key, material));
            }
        }
        // Two tries: when another engine records the key first, the second
        // try reads theirs.
        for _ in 0..2 {
            if let Some(record) = self.records.newest(&id).await? {
                let key = KeyRef {
                    id,
                    generation: record.generation,
                };
                let material = self
                    .under_root(key, layer, Some(record))
                    .await?
                    .ok_or_else(failure::mismatch)?;
                self.saw_newest(key);
                return Ok((key, material));
            }
            let key = KeyRef {
                id,
                generation: Generation::FIRST,
            };
            let material = KeyMaterial::generate(self.random.as_ref())?;
            let wrapped = self.store.wrap(&material, &root_binding(key, layer)).await?;
            let record = KeyRecord {
                id,
                layer,
                generation: key.generation,
                wrapped,
                wrapped_by: KeyRef {
                    id: KeyId::root(),
                    generation: Generation::FIRST,
                },
            };
            match self.records.insert(record).await {
                Ok(()) => {
                    self.saw_newest(key);
                    return Ok((key, self.cache(key, material)));
                }
                Err(conflict) if conflict.code() == &failure::KEY_RECORD_CONFLICT => {}
                Err(other) => return Err(other),
            }
        }
        Err(failure::record_conflict())
    }

    /// An owner's key at one generation, or `None` when no record of it
    /// exists.
    async fn owner_key(
        &self,
        workspace: &WorkspaceId,
        key: KeyRef,
    ) -> Result<Option<Arc<KeyMaterial>>, Failure> {
        if let Some(material) = self.cached(key) {
            return Ok(Some(material));
        }
        match self.records.get(&key.id, key.generation).await? {
            Some(record) => self.unwrap_owner(workspace, key, &record).await,
            None => Ok(None),
        }
    }

    async fn unwrap_owner(
        &self,
        workspace: &WorkspaceId,
        key: KeyRef,
        record: &KeyRecord,
    ) -> Result<Option<Arc<KeyMaterial>>, Failure> {
        // The workspace is taken from the context, never from the record:
        // a record moved from another workspace must not open.
        let above = KeyRef {
            id: KeyId::workspace(workspace),
            generation: record.wrapped_by.generation,
        };
        let Some(workspace_key) = self.under_root(above, Layer::Workspace, None).await? else {
            return Ok(None);
        };
        let material = cipher::unwrap(
            &workspace_key,
            record.wrapped.as_bytes(),
            owner_bound_to(key, above).as_bytes(),
        )?;
        Ok(Some(self.cache(key, material)))
    }

    /// The newest generation of an owner's key, made and recorded when the
    /// owner has none.
    async fn newest_owner_key(
        &self,
        workspace: &WorkspaceId,
        id: KeyId,
    ) -> Result<(KeyRef, Arc<KeyMaterial>), Failure> {
        if let Some(generation) = self.newest_seen(&id) {
            let key = KeyRef { id, generation };
            if let Some(material) = self.cached(key) {
                return Ok((key, material));
            }
        }
        for _ in 0..2 {
            if let Some(record) = self.records.newest(&id).await? {
                let key = KeyRef {
                    id,
                    generation: record.generation,
                };
                let material = self
                    .unwrap_owner(workspace, key, &record)
                    .await?
                    .ok_or_else(failure::mismatch)?;
                self.saw_newest(key);
                return Ok((key, material));
            }
            let (above, workspace_key) = self
                .newest_under_root(KeyId::workspace(workspace), Layer::Workspace)
                .await?;
            let key = KeyRef {
                id,
                generation: Generation::FIRST,
            };
            let material = KeyMaterial::generate(self.random.as_ref())?;
            let wrapped = cipher::wrap(
                &workspace_key,
                &material,
                owner_bound_to(key, above).as_bytes(),
                self.random.as_ref(),
            )?;
            let record = KeyRecord {
                id,
                layer: Layer::Owner,
                generation: key.generation,
                wrapped,
                wrapped_by: above,
            };
            match self.records.insert(record).await {
                Ok(()) => {
                    self.saw_newest(key);
                    return Ok((key, self.cache(key, material)));
                }
                Err(conflict) if conflict.code() == &failure::KEY_RECORD_CONFLICT => {}
                Err(other) => return Err(other),
            }
        }
        Err(failure::record_conflict())
    }
}

impl std::fmt::Debug for Vault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vault")
            .field("store", &self.store.describe())
            .finish_non_exhaustive()
    }
}

fn root_binding(key: KeyRef, layer: Layer) -> Binding {
    Binding {
        key: key.id,
        layer,
        generation: key.generation,
    }
}

/// What is authenticated with an owner's key wrapped by a workspace's key:
/// which key it is, and which key wrapped it.
fn owner_bound_to(key: KeyRef, above: KeyRef) -> BoundTo {
    BoundTo::new(UPPER_KEY)
        .field("key", key.id.as_bytes())
        .field("layer", Layer::Owner.as_str().as_bytes())
        .number("generation", key.generation.get())
        .field("wrapped_by", above.id.as_bytes())
        .number("wrapped_by_generation", above.generation.get())
}
