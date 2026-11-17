//! The failures this crate produces: one class and one code each.
//!
//! A failure from this crate never names a key, a value, or a byte of either,
//! and a mismatch never says which of its reasons applied.

use a2m_core::{Failure, FailureClass, FailureCode};

/// The key store does not answer. The kernel is degraded until it does.
pub const KEY_STORE_UNREACHABLE: FailureCode = FailureCode::fixed("key_store_unreachable");
/// The root that wrapped a key is no longer in the key store.
pub const KEY_STORE_ENTRY_MISSING: FailureCode = FailureCode::fixed("key_store_entry_missing");
/// A sealed item does not open under the context given.
pub const SEALED_ITEM_MISMATCH: FailureCode = FailureCode::fixed("sealed_item_mismatch");
/// A sealed item's format or cipher is one this engine does not know.
pub const SEALED_FORMAT_UNSUPPORTED: FailureCode = FailureCode::fixed("sealed_format_unsupported");
/// The passphrase route was asked for and is not switched on.
pub const VAULT_PASSPHRASE_NOT_ENABLED: FailureCode =
    FailureCode::fixed("vault_passphrase_not_enabled");
/// The passphrase given is not the one the keys were wrapped under.
pub const VAULT_PASSPHRASE_MISMATCH: FailureCode = FailureCode::fixed("vault_passphrase_mismatch");
/// A key record already exists, or changed before it could be replaced.
pub const KEY_RECORD_CONFLICT: FailureCode = FailureCode::fixed("key_record_conflict");
/// The operating system gave no random bytes, so no key or nonce was made.
pub const RANDOMNESS_UNAVAILABLE: FailureCode = FailureCode::fixed("randomness_unavailable");

/// The key store did not answer. `what` is a fixed phrase, never text from
/// the key store itself.
pub(crate) fn unreachable(what: &'static str) -> Failure {
    Failure::new(
        FailureClass::Busy,
        KEY_STORE_UNREACHABLE,
        format!("The key store is not reachable: {what}."),
        "Unlock or start the key store and try again. Nothing new can be sealed or opened until it answers; run `anything2mcp doctor` to see which key store is in use.",
    )
}

/// The root that wrapped a key is gone.
pub(crate) fn entry_missing() -> Failure {
    Failure::new(
        FailureClass::Busy,
        KEY_STORE_ENTRY_MISSING,
        "The root key that protects this workspace's keys is no longer in the key store.",
        "Saved sessions and stored evidence sealed under it cannot be read. Sign in again; saved sessions must be created anew.",
    )
}

/// An envelope did not open. One message for every reason.
pub(crate) fn mismatch() -> Failure {
    Failure::new(
        FailureClass::Input,
        SEALED_ITEM_MISMATCH,
        "The sealed item does not open under the context given.",
        "Open it with the workspace, owner, kind, and item it was sealed for. If those are right, the sealed item is damaged and must be made again.",
    )
}

/// An envelope of a format or cipher this engine does not know.
pub(crate) fn unsupported() -> Failure {
    Failure::new(
        FailureClass::Input,
        SEALED_FORMAT_UNSUPPORTED,
        "The sealed item has a format or cipher this engine does not know.",
        "Open it with the engine version that sealed it, or a newer one.",
    )
}

/// The passphrase route is not switched on.
pub(crate) fn passphrase_not_enabled() -> Failure {
    Failure::new(
        FailureClass::Policy,
        VAULT_PASSPHRASE_NOT_ENABLED,
        "The passphrase route for the root key is not switched on.",
        "Use the operating system's key store or a key service. A person may switch the passphrase route on as a protected setting; it is weaker than either.",
    )
}

/// The passphrase is not the one the keys were wrapped under.
pub(crate) fn passphrase_mismatch() -> Failure {
    Failure::new(
        FailureClass::Input,
        VAULT_PASSPHRASE_MISMATCH,
        "The passphrase given is not the one this installation's keys were wrapped under.",
        "Start again with the passphrase used before. Without it, saved sessions and stored evidence cannot be read.",
    )
}

/// A key record exists already, or changed underneath a replacement.
pub(crate) fn record_conflict() -> Failure {
    Failure::new(
        FailureClass::Busy,
        KEY_RECORD_CONFLICT,
        "The key record was written by someone else first; nothing changed.",
        "Read the record again and retry.",
    )
}

/// No random bytes.
pub(crate) fn no_randomness() -> Failure {
    Failure::new(
        FailureClass::Busy,
        RANDOMNESS_UNAVAILABLE,
        "The operating system gave no random bytes, so no key was made.",
        "Try again. If it persists, the machine's random source is not working.",
    )
}

/// Text that is not a name this crate accepts.
pub(crate) fn bad_name(what: &'static str) -> Failure {
    Failure::new(
        FailureClass::Input,
        FailureCode::SCHEMA_INVALID,
        format!("A {what} is 1 to 255 bytes of text with no control characters; this one is not."),
        "Give a name of that shape.",
    )
}
