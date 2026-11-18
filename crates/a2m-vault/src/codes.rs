//! The failure codes this crate produces, each with its one class. A failure
//! from this crate never names a key, a value, or a byte of either.
//!
//! Whether a `busy` code passes by itself, and why:
//!
//! - `key_store_unreachable` does not: the key store is locked, stopped, or
//!   refusing, and a person unlocks or starts it.
//! - `key_store_entry_missing` does not: the root key is gone for good.
//! - `key_record_conflict` does: another writer won a race for the record,
//!   and a new call reads what that writer wrote before it writes.
//! - `randomness_unavailable` does not: the operating system's random source
//!   waits until it is ready rather than failing, so a failure is a source
//!   that is broken or refused to the engine.

a2m_core::failure_codes! {
    KEY_STORE_UNREACHABLE = Busy, "key_store_unreachable",
        meaning "The key store does not answer. The kernel is degraded until it does.",
        next "Unlock or start the key store, then try again; run `anything2mcp doctor` to see which key store is in use.",
        passes not_by_itself;
    KEY_STORE_ENTRY_MISSING = Busy, "key_store_entry_missing",
        meaning "The root that wrapped a key is no longer in the key store.",
        next "Sign in again to make each saved session anew; what was sealed under the lost root cannot be read.",
        passes not_by_itself;
    SEALED_ITEM_MISMATCH = Input, "sealed_item_mismatch",
        meaning "A sealed item does not open under the context given.",
        next "Open it with the workspace, owner, kind, and item it was sealed for; if those are right, make the sealed item again.";
    SEALED_FORMAT_UNSUPPORTED = Input, "sealed_format_unsupported",
        meaning "A sealed item's format or cipher is one this engine does not know.",
        next "Open it with the engine version that sealed it, or a newer one.";
    VAULT_PASSPHRASE_NOT_ENABLED = Policy, "vault_passphrase_not_enabled",
        meaning "The passphrase route was asked for and is not switched on.",
        next "Use the operating system's key store or a key service, or have a person choose the passphrase route in the installation's settings file.";
    VAULT_PASSPHRASE_MISMATCH = Input, "vault_passphrase_mismatch",
        meaning "The passphrase given is not the one the keys were wrapped under.",
        next "Start again with the passphrase used before.";
    VAULT_PASSPHRASE_TOO_SHORT = Input, "vault_passphrase_too_short",
        meaning "The passphrase given is too short to derive a root key from.",
        next "Start again with a passphrase of at least 16 bytes.";
    VAULT_PASSPHRASE_MISSING = Input, "vault_passphrase_missing",
        meaning "The passphrase route is chosen and no passphrase was given.",
        next "Set the environment variable the message names to the passphrase, then run the command again.";
    KEY_RECORD_CONFLICT = Busy, "key_record_conflict",
        meaning "A key record already exists, or changed before it could be replaced.",
        next "Make the call again; it reads the record that was written first.",
        passes by_itself;
    RANDOMNESS_UNAVAILABLE = Busy, "randomness_unavailable",
        meaning "The operating system gave no random bytes, so no key or nonce was made.",
        next "Make the machine's random source available to the engine, then try again.",
        passes not_by_itself;
}
