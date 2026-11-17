//! The operating system's key store: Credential Manager on Windows, the
//! Keychain on macOS, and the Secret Service on a Linux desktop.
//!
//! The store holds one entry per installation: the root and its identifier.
//! Keys are wrapped in memory under that root. The outside crates that reach
//! each store are synchronous, so every call to them runs on a blocking
//! thread.

use std::sync::{Arc, Mutex, PoisonError};

use a2m_core::Failure;
use keyring_core::{CredentialStore, Entry, Error};
use zeroize::Zeroizing;

use super::{BoxFuture, KeyStore, LocalRoot, StoreDescription, StoreHealth, StoreStrength};
use crate::failure;
use crate::material::{KeyMaterial, OsRandom, RandomSource, Wrapped};
use crate::names::{Binding, InstallationName};

/// The service name every entry of this product carries in the key store.
const SERVICE: &str = "anything2mcp";

/// The root back end over the operating system's key store.
pub struct OsKeyStore {
    name: &'static str,
    entry: Arc<Entry>,
    root: Mutex<Option<Arc<LocalRoot>>>,
    random: Arc<dyn RandomSource>,
}

impl OsKeyStore {
    /// Connects to this machine's key store and names the installation's
    /// entry in it. Nothing is read or written yet.
    ///
    /// # Errors
    ///
    /// `busy`/`key_store_unreachable` when this machine has no key store this
    /// build can reach, as on Linux without a desktop session or without the
    /// `secret-service` feature, or when the store does not answer.
    pub async fn open(installation: &InstallationName) -> Result<Self, Failure> {
        let (name, store) = blocking(platform_store).await??;
        Self::over(name, store, installation, Arc::new(OsRandom)).await
    }

    /// The same back end over any credential store of the outside crate
    /// family. Tests use it with that family's in-memory store.
    pub(crate) async fn over(
        name: &'static str,
        store: Arc<CredentialStore>,
        installation: &InstallationName,
        random: Arc<dyn RandomSource>,
    ) -> Result<Self, Failure> {
        let user = installation.as_str().to_owned();
        let entry = blocking(move || store.build(SERVICE, &user, None))
            .await?
            .map_err(|error| store_failure(&error))?;
        Ok(Self {
            name,
            entry: Arc::new(entry),
            root: Mutex::new(None),
            random,
        })
    }

    fn cached(&self) -> Option<Arc<LocalRoot>> {
        self.root
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn cache(&self, root: LocalRoot) -> Arc<LocalRoot> {
        let root = Arc::new(root);
        *self.root.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::clone(&root));
        root
    }

    /// Reads the entry. `Ok(None)` when the store answers and has no entry.
    async fn read(&self) -> Result<Option<LocalRoot>, Failure> {
        let entry = Arc::clone(&self.entry);
        let read = blocking(move || entry.get_secret().map(Zeroizing::new)).await?;
        match read {
            Ok(bytes) => LocalRoot::from_entry(&bytes).map(Some).ok_or_else(|| {
                failure::unreachable(
                    "its entry for this installation is not a root this engine can read",
                )
            }),
            Err(Error::NoEntry) => Ok(None),
            Err(error) => Err(store_failure(&error)),
        }
    }

    /// The root, read from the store the first time and kept in memory after.
    async fn root(&self) -> Result<Option<Arc<LocalRoot>>, Failure> {
        if let Some(root) = self.cached() {
            return Ok(Some(root));
        }
        Ok(self.read().await?.map(|root| self.cache(root)))
    }

    /// The root, made and written to the store when the store has none.
    async fn root_or_new(&self) -> Result<Arc<LocalRoot>, Failure> {
        if let Some(root) = self.root().await? {
            return Ok(root);
        }
        let made = LocalRoot::generate(self.random.as_ref())?;
        let bytes = made.to_entry();
        let entry = Arc::clone(&self.entry);
        blocking(move || entry.set_secret(&bytes))
            .await?
            .map_err(|error| store_failure(&error))?;
        // Use what the store now holds, so that two engines started at the
        // same moment end with one root between them.
        let stored = self.read().await?.ok_or_else(failure::entry_missing)?;
        Ok(self.cache(stored))
    }

    /// Removes the installation's entry from the store.
    #[cfg(test)]
    pub(crate) async fn delete_entry(&self) -> Result<(), Failure> {
        let entry = Arc::clone(&self.entry);
        let deleted = blocking(move || entry.delete_credential()).await?;
        *self.root.lock().unwrap_or_else(PoisonError::into_inner) = None;
        match deleted {
            Ok(()) | Err(Error::NoEntry) => Ok(()),
            Err(error) => Err(store_failure(&error)),
        }
    }
}

impl KeyStore for OsKeyStore {
    fn describe(&self) -> StoreDescription {
        StoreDescription {
            name: self.name.to_owned(),
            strength: StoreStrength::OsStore,
        }
    }

    fn wrap<'a>(
        &'a self,
        key: &'a KeyMaterial,
        bound_to: &'a Binding,
    ) -> BoxFuture<'a, Result<Wrapped, Failure>> {
        Box::pin(async move {
            self.root_or_new()
                .await?
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
                .await?
                .ok_or_else(failure::entry_missing)?
                .unwrap(wrapped, bound_to, failure::entry_missing)
        })
    }

    fn health(&self) -> BoxFuture<'_, StoreHealth> {
        Box::pin(async move {
            match self.read().await {
                Ok(Some(_)) => StoreHealth::Ok,
                Ok(None) => StoreHealth::EntryMissing,
                Err(_) => StoreHealth::Unreachable,
            }
        })
    }
}

/// Runs a synchronous key-store call off the async threads.
async fn blocking<T: Send + 'static>(
    call: impl FnOnce() -> T + Send + 'static,
) -> Result<T, Failure> {
    tokio::task::spawn_blocking(call)
        .await
        .map_err(|_| failure::unreachable("the call to it did not finish"))
}

/// A failure for an error of the outside key-store crates. Only the kind of
/// error is used: its text and any bytes it carries are dropped unread.
fn store_failure(error: &Error) -> Failure {
    match error {
        Error::NoEntry => failure::entry_missing(),
        Error::NoStorageAccess(_) => failure::unreachable("it is locked or refused access"),
        Error::PlatformFailure(_) => {
            failure::unreachable("the operating system reported a failure")
        }
        _ => failure::unreachable("it refused the request"),
    }
}

#[cfg(target_os = "macos")]
fn platform_store() -> Result<(&'static str, Arc<CredentialStore>), Failure> {
    let store: Arc<CredentialStore> = apple_native_keyring_store::keychain::Store::new()
        .map_err(|error| store_failure(&error))?;
    Ok(("macOS Keychain", store))
}

#[cfg(target_os = "windows")]
fn platform_store() -> Result<(&'static str, Arc<CredentialStore>), Failure> {
    let store: Arc<CredentialStore> =
        windows_native_keyring_store::Store::new().map_err(|error| store_failure(&error))?;
    Ok(("Windows Credential Manager", store))
}

#[cfg(all(target_os = "linux", feature = "secret-service"))]
fn platform_store() -> Result<(&'static str, Arc<CredentialStore>), Failure> {
    let store: Arc<CredentialStore> =
        zbus_secret_service_keyring_store::Store::new().map_err(|error| store_failure(&error))?;
    Ok(("Secret Service", store))
}

#[cfg(not(any(
    target_os = "macos",
    target_os = "windows",
    all(target_os = "linux", feature = "secret-service")
)))]
fn platform_store() -> Result<(&'static str, Arc<CredentialStore>), Failure> {
    Err(failure::unreachable(
        "this build has no operating-system key store for this machine",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::names::{Generation, KeyId, Layer};

    fn binding() -> Binding {
        Binding {
            key: KeyId::root(),
            layer: Layer::Workspace,
            generation: Generation::FIRST,
        }
    }

    async fn over(store: &Arc<CredentialStore>, installation: &str) -> OsKeyStore {
        OsKeyStore::over(
            "test store",
            Arc::clone(store),
            &InstallationName::new(installation).unwrap(),
            Arc::new(OsRandom),
        )
        .await
        .unwrap()
    }

    /// The back end's own logic, against the outside crate family's
    /// in-memory store, which runs on every machine.
    #[tokio::test]
    async fn the_root_lives_in_the_store_and_survives_a_restart() {
        let store: Arc<CredentialStore> = keyring_core::mock::Store::new().unwrap();
        let key = KeyMaterial::generate(&OsRandom).unwrap();

        let first = over(&store, "example-installation").await;
        assert_eq!(first.health().await, StoreHealth::EntryMissing);
        let missing = first
            .unwrap(&Wrapped::from_bytes(vec![1; 90]), &binding())
            .await;
        assert_eq!(
            missing.unwrap_err().code(),
            &failure::KEY_STORE_ENTRY_MISSING
        );
        let wrapped = first.wrap(&key, &binding()).await.unwrap();
        assert_eq!(first.health().await, StoreHealth::Ok);
        assert_eq!(first.describe().strength, StoreStrength::OsStore);
        drop(first);

        // A new process: the same entry, read back from the store.
        let second = over(&store, "example-installation").await;
        let unwrapped = second.unwrap(&wrapped, &binding()).await.unwrap();
        assert_eq!(unwrapped.expose(), key.expose());

        // Another installation has another entry and another root.
        let other = over(&store, "other-installation").await;
        other.wrap(&key, &binding()).await.unwrap();
        let foreign = other.unwrap(&wrapped, &binding()).await.unwrap_err();
        assert_eq!(foreign.code(), &failure::KEY_STORE_ENTRY_MISSING);

        // The entry is removed, as when a credential store is reset.
        second.delete_entry().await.unwrap();
        assert_eq!(second.health().await, StoreHealth::EntryMissing);
        let gone = second.unwrap(&wrapped, &binding()).await.unwrap_err();
        assert_eq!(gone.code(), &failure::KEY_STORE_ENTRY_MISSING);

        // A new root is not the old one: the old key stays unreadable.
        second.wrap(&key, &binding()).await.unwrap();
        let replaced = second.unwrap(&wrapped, &binding()).await.unwrap_err();
        assert_eq!(replaced.code(), &failure::KEY_STORE_ENTRY_MISSING);
    }

    /// The real key store of this machine. Ignored by default: a headless
    /// machine has none, and a desktop may ask the person to unlock it.
    /// Run with `cargo test -p a2m-vault -- --ignored`.
    #[tokio::test]
    #[ignore = "needs an operating-system key store and may prompt"]
    async fn the_real_key_store_holds_the_root() {
        let installation = InstallationName::new("anything2mcp-vault-test").unwrap();
        let store = OsKeyStore::open(&installation).await.unwrap();
        store.delete_entry().await.unwrap();
        let key = KeyMaterial::generate(&OsRandom).unwrap();
        let wrapped = store.wrap(&key, &binding()).await.unwrap();

        let again = OsKeyStore::open(&installation).await.unwrap();
        assert_eq!(again.health().await, StoreHealth::Ok);
        let unwrapped = again.unwrap(&wrapped, &binding()).await.unwrap();
        assert_eq!(unwrapped.expose(), key.expose());
        again.delete_entry().await.unwrap();
        assert_eq!(again.health().await, StoreHealth::EntryMissing);
    }

    #[cfg(not(any(
        target_os = "macos",
        target_os = "windows",
        all(target_os = "linux", feature = "secret-service")
    )))]
    #[tokio::test]
    async fn a_machine_with_no_key_store_says_so() {
        let installation = InstallationName::new("example-installation").unwrap();
        let refused = OsKeyStore::open(&installation).await.err().unwrap();
        assert_eq!(refused.code(), &failure::KEY_STORE_UNREACHABLE);
    }
}
