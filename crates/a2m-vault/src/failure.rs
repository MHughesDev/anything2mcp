//! The failures this crate produces: one class and one code each.
//!
//! A failure from this crate never names a key, a value, or a byte of either,
//! and a mismatch never says which of its reasons applied.

use a2m_core::{Failure, FailureClass, FailureCode};

pub use crate::codes::{
    KEY_RECORD_CONFLICT, KEY_STORE_ENTRY_MISSING, KEY_STORE_UNREACHABLE, RANDOMNESS_UNAVAILABLE,
    SEALED_FORMAT_UNSUPPORTED, SEALED_ITEM_MISMATCH, VAULT_PASSPHRASE_MISMATCH,
    VAULT_PASSPHRASE_MISSING, VAULT_PASSPHRASE_NOT_ENABLED, VAULT_PASSPHRASE_TOO_SHORT,
};

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
        "The passphrase route for the root key is not chosen.",
        "Use the operating system's key store or a key service. A person may choose the passphrase route in the installation's settings file, as `backend = \"passphrase\"` under `[vault]`; it is weaker than either.",
    )
}

/// The route is chosen and no passphrase was given. `variable` is the name
/// of the environment variable, never a value.
pub(crate) fn passphrase_missing(variable: &str) -> Failure {
    Failure::new(
        FailureClass::Input,
        VAULT_PASSPHRASE_MISSING,
        "The vault takes its root key from a passphrase, and no passphrase was given to the engine, so nothing can be sealed or opened. The passphrase protects the sessions and secrets the engine keeps on this machine: a copy of its files opens nothing without it.",
        format!(
            "Set the environment variable `{variable}` to a passphrase of at least 16 bytes in the shell you run `anything2mcp` from, then run the command again; the command hands it to the engine it starts. Choose it once and keep it, in a password manager: what was kept under one passphrase opens under no other."
        ),
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

/// The passphrase is shorter than the floor. The message never holds it.
pub(crate) fn passphrase_too_short() -> Failure {
    Failure::new(
        FailureClass::Input,
        VAULT_PASSPHRASE_TOO_SHORT,
        "The passphrase for the root key is shorter than 16 bytes, so no key was derived from it.",
        "Start again with a passphrase of at least 16 bytes: several unrelated words, or random characters from a password manager. A short one can be found by trying every candidate against the stored keys.",
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
