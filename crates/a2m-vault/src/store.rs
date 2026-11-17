//! Where the root lives: the `KeyStore` interface and its back ends.
//!
//! A key store does three things: it wraps a key under the root, it unwraps
//! one, and it reports its health. A desktop back end holds the root in the
//! operating system's store and wraps in memory; a key service wraps remotely
//! and never releases the root.

mod creation;
mod memory;
mod os;
mod passphrase;

use std::future::Future;
use std::pin::Pin;

use a2m_core::Failure;
use zeroize::Zeroizing;

use crate::cipher::{self, BoundTo};
use crate::failure;
use crate::material::{KeyMaterial, RandomSource, Wrapped};
use crate::names::Binding;

pub use memory::MemoryKeyStore;
pub use os::OsKeyStore;
pub use passphrase::{PassphraseKeyStore, PassphraseRoute, PassphraseSalt};

/// A future a trait object can return. The interfaces of this crate are used
/// as trait objects, so their asynchronous methods return this.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The root back end.
///
/// `wrap` and `unwrap` are the only ways the root is ever used. An
/// implementation never returns, prints, or writes the root.
pub trait KeyStore: Send + Sync {
    /// Which key store this is and how strong, for `doctor`.
    fn describe(&self) -> StoreDescription;

    /// Wraps `key` under the root, bound to what it is.
    ///
    /// A local back end that holds no root yet makes one here: a first key
    /// wrapped under a new root loses nothing.
    fn wrap<'a>(
        &'a self,
        key: &'a KeyMaterial,
        bound_to: &'a Binding,
    ) -> BoxFuture<'a, Result<Wrapped, Failure>>;

    /// Unwraps a key the root wrapped. Never makes a root: when the root that
    /// wrapped the key is gone the failure is `busy`/`key_store_entry_missing`.
    fn unwrap<'a>(
        &'a self,
        wrapped: &'a Wrapped,
        bound_to: &'a Binding,
    ) -> BoxFuture<'a, Result<KeyMaterial, Failure>>;

    /// Whether the back end answers and holds a root.
    fn health(&self) -> BoxFuture<'_, StoreHealth>;
}

/// Which key store is in use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreDescription {
    /// A plain name for `doctor`, such as the operating system's name for its
    /// store.
    pub name: String,
    /// How strong the store is.
    pub strength: StoreStrength,
}

/// How strong a key store is. `doctor` warns about the weaker ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StoreStrength {
    /// The operating system's credential store.
    OsStore,
    /// An external key service that never releases the root.
    KeyService,
    /// A root derived from a passphrase given at start. Weaker than either
    /// above.
    Passphrase,
    /// A root held in this process's memory and lost when it ends. For tests
    /// only.
    Memory,
}

impl StoreStrength {
    /// The strength as it is written.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OsStore => "os_store",
            Self::KeyService => "key_service",
            Self::Passphrase => "passphrase",
            Self::Memory => "memory",
        }
    }

    /// Whether `doctor` should warn that this store is weaker than the
    /// operating system's store or a key service.
    #[must_use]
    pub const fn is_weaker(self) -> bool {
        matches!(self, Self::Passphrase | Self::Memory)
    }
}

/// The state of a key store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StoreHealth {
    /// The store answers and holds a root.
    Ok,
    /// The store does not answer.
    Unreachable,
    /// The store answers and holds no root entry: either nothing was sealed
    /// yet on this installation, or the entry was removed.
    EntryMissing,
}

impl StoreHealth {
    /// The state as it is written.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Unreachable => "unreachable",
            Self::EntryMissing => "entry_missing",
        }
    }
}

/// A root held in this process, with the random identifier that tells one
/// root from another.
///
/// The identifier travels in every key the root wraps. When the root is
/// replaced, as when a credential store is reset, a key wrapped by the old
/// one is recognized as such instead of failing as a damaged key.
pub(crate) struct LocalRoot {
    id: [u8; LocalRoot::ID_LENGTH],
    key: KeyMaterial,
}

impl LocalRoot {
    const ID_LENGTH: usize = 16;
    /// The format of a wrapped key and of the entry that holds the root.
    const FORMAT: u8 = 1;

    /// A new root from `random`.
    pub(crate) fn generate(random: &dyn RandomSource) -> Result<Self, Failure> {
        let mut id = [0u8; Self::ID_LENGTH];
        random.fill(&mut id)?;
        Ok(Self {
            id,
            key: KeyMaterial::generate(random)?,
        })
    }

    /// A root from derived bytes: the key, then the identifier.
    pub(crate) fn from_derived(bytes: &[u8]) -> Option<Self> {
        let (key, id) = bytes.split_at_checked(KeyMaterial::LENGTH)?;
        Some(Self {
            id: id.try_into().ok()?,
            key: KeyMaterial::from_slice(key).ok()?,
        })
    }

    /// The bytes kept in the key store's entry: format, identifier, key.
    pub(crate) fn to_entry(&self) -> Zeroizing<Vec<u8>> {
        let mut entry = Zeroizing::new(Vec::with_capacity(
            1 + Self::ID_LENGTH + KeyMaterial::LENGTH,
        ));
        entry.push(Self::FORMAT);
        entry.extend_from_slice(&self.id);
        entry.extend_from_slice(self.key.expose());
        entry
    }

    /// Reads what [`LocalRoot::to_entry`] wrote, or `None` when the entry is
    /// not a root of a format this engine knows.
    pub(crate) fn from_entry(entry: &[u8]) -> Option<Self> {
        let (format, rest) = entry.split_first()?;
        if *format != Self::FORMAT {
            return None;
        }
        let (id, key) = rest.split_first_chunk::<{ Self::ID_LENGTH }>()?;
        Some(Self {
            id: *id,
            key: KeyMaterial::from_slice(key).ok()?,
        })
    }

    fn bound_to(&self, bound_to: &Binding) -> BoundTo {
        bound_to.bind(BoundTo::new("a2m-vault/root-wrap/1").field("root", &self.id))
    }

    /// Wraps `key`: format, this root's identifier, nonce, ciphertext.
    pub(crate) fn wrap(
        &self,
        key: &KeyMaterial,
        bound_to: &Binding,
        random: &dyn RandomSource,
    ) -> Result<Wrapped, Failure> {
        let inner = cipher::wrap(&self.key, key, self.bound_to(bound_to).as_bytes(), random)?;
        let mut bytes = Vec::with_capacity(1 + Self::ID_LENGTH + inner.as_bytes().len());
        bytes.push(Self::FORMAT);
        bytes.extend_from_slice(&self.id);
        bytes.extend_from_slice(inner.as_bytes());
        Ok(Wrapped::from_bytes(bytes))
    }

    /// Unwraps a key this root wrapped. `other_root` is the failure for a key
    /// that a different root wrapped.
    pub(crate) fn unwrap(
        &self,
        wrapped: &Wrapped,
        bound_to: &Binding,
        other_root: fn() -> Failure,
    ) -> Result<KeyMaterial, Failure> {
        let (format, rest) = wrapped
            .as_bytes()
            .split_first()
            .ok_or_else(failure::mismatch)?;
        if *format != Self::FORMAT {
            return Err(failure::unsupported());
        }
        let (id, inner) = rest
            .split_first_chunk::<{ Self::ID_LENGTH }>()
            .ok_or_else(failure::mismatch)?;
        if *id != self.id {
            return Err(other_root());
        }
        cipher::unwrap(&self.key, inner, self.bound_to(bound_to).as_bytes())
    }
}
