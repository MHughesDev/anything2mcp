//! Envelope encryption, key-store back ends, and the redactor.
//!
//! The vault turns plain bytes into sealed bytes and back, under four key
//! layers: a root per installation, a key per workspace, a key per owner, and
//! a data key per item. It stores nothing itself. [`Vault::seal`] returns a
//! [`Sealed`] value that the caller keeps; wrapped workspace and owner keys
//! are kept through [`KeyRecords`], which the store implements; the root
//! stays in its [`KeyStore`]. The [`Redactor`] removes every secret the
//! vault released from any text on its way out.
//!
//! This crate writes no cryptography of its own: the cipher and the
//! passphrase derivation come from vetted libraries (decision 0074).
//!
//! The crates this one may depend on are set by the layering table in
//! `docs/reference/technical-reference.md`: `a2m-core`.

mod cipher;
mod digest;
mod failure;
mod hex;
mod material;
mod names;
mod records;
mod redactor;
mod sealed;
mod store;
mod vault;

pub use digest::VaultDigest;
pub use material::{KeyMaterial, Opened, OsRandom, RandomSource, Wrapped};
pub use names::{
    Binding, Generation, InstallationName, ItemId, KeyId, KeyRef, Layer, OwnerKind, OwnerRef,
    SealContext, SealedKind,
};
pub use records::{KeyRecord, KeyRecords, MemoryKeyRecords, key_record_conflict};
pub use redactor::{RedactStream, Redactor, RedactorHealth};
pub use sealed::Sealed;
pub use store::{
    BACKEND_OS_KEY_STORE, BACKEND_PASSPHRASE, BoxFuture, DEFAULT_BACKEND, KeyStore, MemoryKeyStore, OsKeyStore, PassphraseKeyStore, PassphraseRoute,
    PassphraseSalt, StoreDescription, StoreHealth, StoreStrength,
};
pub use vault::{Vault, VaultHealth};

/// The failure codes this crate produces. Each belongs to exactly one class.
pub mod codes {
    pub use crate::failure::{
        KEY_RECORD_CONFLICT, KEY_STORE_ENTRY_MISSING, KEY_STORE_UNREACHABLE,
        RANDOMNESS_UNAVAILABLE, SEALED_FORMAT_UNSUPPORTED, SEALED_ITEM_MISMATCH,
        VAULT_PASSPHRASE_MISMATCH, VAULT_PASSPHRASE_MISSING, VAULT_PASSPHRASE_NOT_ENABLED,
        VAULT_PASSPHRASE_TOO_SHORT,
    };
}
