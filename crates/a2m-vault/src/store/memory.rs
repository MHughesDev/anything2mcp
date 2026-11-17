//! A key store that holds its root in this process's memory. For tests.

use std::sync::{Arc, Mutex, PoisonError};

use a2m_core::Failure;

use super::{BoxFuture, KeyStore, LocalRoot, StoreDescription, StoreHealth, StoreStrength};
use crate::failure;
use crate::material::{KeyMaterial, OsRandom, RandomSource, Wrapped};
use crate::names::Binding;

/// A key store whose root lives in memory and is lost with the process.
///
/// It stands in for the operating system's store in tests: keep the same
/// `MemoryKeyStore` across a simulated restart and the root is still there;
/// call [`MemoryKeyStore::remove_root`] and it is gone, as when a credential
/// store is reset. It is reported as the weakest store and must never hold a
/// real installation's root.
pub struct MemoryKeyStore {
    root: Mutex<Option<Arc<LocalRoot>>>,
    random: Arc<dyn RandomSource>,
}

impl MemoryKeyStore {
    /// An empty store. The root is made at the first `wrap`.
    #[must_use]
    pub fn new() -> Self {
        Self::with_random(Arc::new(OsRandom))
    }

    /// An empty store that takes its random bytes from `random`.
    #[must_use]
    pub fn with_random(random: Arc<dyn RandomSource>) -> Self {
        Self {
            root: Mutex::new(None),
            random,
        }
    }

    /// Removes the root, as a reset credential store would.
    pub fn remove_root(&self) {
        *self.root.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }

    fn root(&self) -> Option<Arc<LocalRoot>> {
        self.root
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn root_or_new(&self) -> Result<Arc<LocalRoot>, Failure> {
        let mut root = self.root.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(root) = root.as_ref() {
            return Ok(Arc::clone(root));
        }
        let made = Arc::new(LocalRoot::generate(self.random.as_ref())?);
        *root = Some(Arc::clone(&made));
        Ok(made)
    }
}

impl Default for MemoryKeyStore {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyStore for MemoryKeyStore {
    fn describe(&self) -> StoreDescription {
        StoreDescription {
            name: "memory (for tests)".to_owned(),
            strength: StoreStrength::Memory,
        }
    }

    fn wrap<'a>(
        &'a self,
        key: &'a KeyMaterial,
        bound_to: &'a Binding,
    ) -> BoxFuture<'a, Result<Wrapped, Failure>> {
        Box::pin(async move {
            self.root_or_new()?
                .wrap(key, bound_to, self.random.as_ref())
        })
    }

    fn unwrap<'a>(
        &'a self,
        wrapped: &'a Wrapped,
        bound_to: &'a Binding,
    ) -> BoxFuture<'a, Result<KeyMaterial, Failure>> {
        Box::pin(async move {
            self.root()
                .ok_or_else(failure::entry_missing)?
                .unwrap(wrapped, bound_to, failure::entry_missing)
        })
    }

    fn health(&self) -> BoxFuture<'_, StoreHealth> {
        Box::pin(async move {
            if self.root().is_some() {
                StoreHealth::Ok
            } else {
                StoreHealth::EntryMissing
            }
        })
    }
}
