//! The failure codes this crate can produce. Each has exactly one class.

use a2m_core::FailureCode;

/// Class `busy`: the log folder cannot be written. The engine keeps running
/// and logs go to standard error, redacted.
pub const STORAGE_FULL: FailureCode = FailureCode::fixed("storage_full");

/// Class `input`: a finished trace still held a registered secret at the
/// check on the way out. The trace is dropped and never reaches the store.
pub const TRACE_HOLDS_SECRET: FailureCode = FailureCode::fixed("trace_holds_secret");

/// Class `input`: the settings given to `start` cannot be used.
pub const TELEMETRY_SETTINGS_INVALID: FailureCode = FailureCode::fixed("telemetry_settings_invalid");
