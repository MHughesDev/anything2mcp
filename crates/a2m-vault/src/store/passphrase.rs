//! The passphrase route: a root derived from a passphrase given at start.
//!
//! For Linux with no desktop session and no key service. It is weaker than
//! the operating system's store, because anyone who learns the passphrase
//! and reads the store can open everything; so it is an explicit opt-in, and
//! `doctor` reports it as weaker. The derivation is Argon2id, which costs
//! memory as well as time for each guess (decision 0074).

use std::fmt;
use std::sync::Arc;

use a2m_core::{Failure, Secret};
use argon2::{Algorithm, Argon2, Params, Version};
use zeroize::Zeroizing;

use super::{BoxFuture, KeyStore, LocalRoot, StoreDescription, StoreHealth, StoreStrength};
use crate::failure;
use crate::material::{KeyMaterial, OsRandom, RandomSource, Wrapped};
use crate::names::Binding;

/// Whether a person switched the passphrase route on. It is a protected
/// setting: the caller reads it from settings that only a person can change,
/// and passes it here, so that choosing this route is always explicit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassphraseRoute {
    /// The route is off. Asking for it is refused.
    NotEnabled,
    /// A person switched the route on.
    Enabled,
}

/// The salt of the derivation: random, not secret, one per installation. The
/// caller keeps it beside the key records and gives the same one at every
/// start.
#[derive(Clone, PartialEq, Eq)]
pub struct PassphraseSalt([u8; PassphraseSalt::LENGTH]);

impl PassphraseSalt {
    /// The number of bytes in a salt.
    pub const LENGTH: usize = 16;

    /// A new salt, for a new installation.
    ///
    /// # Errors
    ///
    /// `busy`/`randomness_unavailable` when the source fails.
    pub fn generate(random: &dyn RandomSource) -> Result<Self, Failure> {
        let mut salt = [0u8; Self::LENGTH];
        random.fill(&mut salt)?;
        Ok(Self(salt))
    }

    /// A salt read back from where it was kept.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; Self::LENGTH]) -> Self {
        Self(bytes)
    }

    /// The salt's bytes, for keeping.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; Self::LENGTH] {
        &self.0
    }
}

impl fmt::Debug for PassphraseSalt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PassphraseSalt(16 bytes)")
    }
}

/// What one derivation costs.
#[derive(Debug, Clone, Copy)]
struct Cost {
    memory_kib: u32,
    passes: u32,
}

/// 64 MiB and three passes on one thread: the second recommended setting of
/// the Argon2 standard, with one lane so that the cost is the same on every
/// machine.
const STANDARD: Cost = Cost {
    memory_kib: 64 * 1024,
    passes: 3,
};

/// The root back end for a passphrase.
pub struct PassphraseKeyStore {
    root: LocalRoot,
    random: Arc<dyn RandomSource>,
}

impl PassphraseKeyStore {
    /// Derives the root from `passphrase` and `salt`. This takes on the order
    /// of a second and 64 MiB of memory, on a blocking thread.
    ///
    /// # Errors
    ///
    /// `policy`/`vault_passphrase_not_enabled` when `route` is not
    /// [`PassphraseRoute::Enabled`]. `busy`/`key_store_unreachable` when the
    /// derivation cannot run, as when the memory for it cannot be had.
    pub async fn derive(
        route: PassphraseRoute,
        passphrase: &Secret,
        salt: &PassphraseSalt,
    ) -> Result<Self, Failure> {
        Self::derive_at(route, passphrase, salt, STANDARD, Arc::new(OsRandom)).await
    }

    async fn derive_at(
        route: PassphraseRoute,
        passphrase: &Secret,
        salt: &PassphraseSalt,
        cost: Cost,
        random: Arc<dyn RandomSource>,
    ) -> Result<Self, Failure> {
        if route != PassphraseRoute::Enabled {
            return Err(failure::passphrase_not_enabled());
        }
        let passphrase = Zeroizing::new(passphrase.expose().as_bytes().to_vec());
        let salt = salt.0;
        let root = tokio::task::spawn_blocking(move || derive_root(&passphrase, &salt, cost))
            .await
            .map_err(|_| failure::unreachable("the passphrase derivation did not finish"))??;
        Ok(Self { root, random })
    }
}

/// Argon2id over the passphrase and salt: 32 bytes of key, then 16 bytes that
/// identify the root, so that a wrong passphrase is recognized as such.
fn derive_root(passphrase: &[u8], salt: &[u8], cost: Cost) -> Result<LocalRoot, Failure> {
    let cannot = || failure::unreachable("the passphrase derivation could not run");
    let mut derived = Zeroizing::new([0u8; KeyMaterial::LENGTH + 16]);
    let params =
        Params::new(cost.memory_kib, cost.passes, 1, Some(derived.len())).map_err(|_| cannot())?;
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(passphrase, salt, derived.as_mut_slice())
        .map_err(|_| cannot())?;
    LocalRoot::from_derived(derived.as_slice()).ok_or_else(cannot)
}

impl KeyStore for PassphraseKeyStore {
    fn describe(&self) -> StoreDescription {
        StoreDescription {
            name: "passphrase given at start".to_owned(),
            strength: StoreStrength::Passphrase,
        }
    }

    fn wrap<'a>(
        &'a self,
        key: &'a KeyMaterial,
        bound_to: &'a Binding,
    ) -> BoxFuture<'a, Result<Wrapped, Failure>> {
        Box::pin(async move { self.root.wrap(key, bound_to, self.random.as_ref()) })
    }

    fn unwrap<'a>(
        &'a self,
        wrapped: &'a Wrapped,
        bound_to: &'a Binding,
    ) -> BoxFuture<'a, Result<KeyMaterial, Failure>> {
        Box::pin(async move {
            self.root
                .unwrap(wrapped, bound_to, failure::passphrase_mismatch)
        })
    }

    fn health(&self) -> BoxFuture<'_, StoreHealth> {
        Box::pin(async { StoreHealth::Ok })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::names::{Generation, KeyId, Layer};

    /// The smallest cost the derivation accepts, so the tests are quick.
    const QUICK: Cost = Cost {
        memory_kib: 8,
        passes: 1,
    };

    fn binding() -> Binding {
        Binding {
            key: KeyId::root(),
            layer: Layer::Workspace,
            generation: Generation::FIRST,
        }
    }

    async fn store(passphrase: &str, salt: &PassphraseSalt) -> PassphraseKeyStore {
        PassphraseKeyStore::derive_at(
            PassphraseRoute::Enabled,
            &Secret::new(passphrase),
            salt,
            QUICK,
            Arc::new(OsRandom),
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn the_route_must_be_switched_on() {
        let salt = PassphraseSalt::from_bytes([7; 16]);
        let refused = PassphraseKeyStore::derive(
            PassphraseRoute::NotEnabled,
            &Secret::new("a long passphrase"),
            &salt,
        )
        .await
        .err()
        .unwrap();
        assert_eq!(refused.code(), &failure::VAULT_PASSPHRASE_NOT_ENABLED);
        assert_eq!(refused.class(), a2m_core::FailureClass::Policy);
    }

    #[tokio::test]
    async fn the_same_passphrase_and_salt_give_the_same_root_and_no_other_does() {
        let salt = PassphraseSalt::from_bytes([7; 16]);
        let key = KeyMaterial::generate(&OsRandom).unwrap();
        let first = store("a long passphrase", &salt).await;
        assert_eq!(first.describe().strength, StoreStrength::Passphrase);
        assert!(first.describe().strength.is_weaker());
        let wrapped = first.wrap(&key, &binding()).await.unwrap();
        drop(first);

        let again = store("a long passphrase", &salt).await;
        let unwrapped = again.unwrap(&wrapped, &binding()).await.unwrap();
        assert_eq!(unwrapped.expose(), key.expose());

        let wrong = store("another passphrase", &salt).await;
        let refused = wrong.unwrap(&wrapped, &binding()).await.unwrap_err();
        assert_eq!(refused.code(), &failure::VAULT_PASSPHRASE_MISMATCH);

        let other_salt = store("a long passphrase", &PassphraseSalt::from_bytes([8; 16])).await;
        let refused = other_salt.unwrap(&wrapped, &binding()).await.unwrap_err();
        assert_eq!(refused.code(), &failure::VAULT_PASSPHRASE_MISMATCH);
    }

    #[test]
    fn the_standard_cost_is_accepted_by_the_derivation() {
        assert!(Params::new(STANDARD.memory_kib, STANDARD.passes, 1, Some(48)).is_ok());
    }
}
